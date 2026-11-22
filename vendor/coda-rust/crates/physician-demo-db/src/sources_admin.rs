//! SQLite-backed practice source connections (`/connections/*`), panel assignment, and invitations.

use crate::driver::DbError;
use crate::schema::{Invitations, PanelMembership, Patients, PracticeConnections};
use crate::store::{DEMO_NOW, DemoStore, not_found};
use physician_wire::source_setup::{
    AccessLossCauseWire, ConnectSourceBody, PracticeConnectionsWire, PracticeSourceWire,
    RequestAssistedSetupBody, SimulateSourceEventBody, SourceConnectionMethodWire,
    SourceOperationResponseWire, SourceStandingWire, SpruceBacklogWire, SupportedSourceWire,
};
use physician_wire::{
    CancelInvitationResponse, ColleagueInvitationResponse, ConnectionStatusWire,
    InviteColleagueRequest, InvitePatientRequest, PanelAssignmentResponse,
    PatientInvitationResponse, PatientLinkWire, PatientWorkspaceResponse,
};
use sea_query::{OnConflict, Order, Query};

const DEMO_PRACTICE_ID: &str = "prac-coda-demo";

impl DemoStore {
    /// Lists all practice source connections and their saved SQLite standings.
    pub async fn connections(&self) -> Result<PracticeConnectionsWire, DbError> {
        self.ensure_ready().await?;
        let q = Query::select()
            .column(PracticeConnections::PayloadJson)
            .from(PracticeConnections::Table)
            .order_by(PracticeConnections::SourceId, Order::Asc)
            .to_owned();
        let mut sources: Vec<PracticeSourceWire> = self
            .driver
            .select(&q)?
            .iter()
            .map(|r| Ok(serde_json::from_str(&r.get_str("payload_json")?)?))
            .collect::<Result<Vec<_>, DbError>>()?;
        sources.sort_by_key(|s| {
            SupportedSourceWire::ALL
                .iter()
                .position(|k| *k == s.source_id)
                .unwrap_or(9)
        });
        Ok(PracticeConnectionsWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            sources,
        })
    }

    /// Connects a practice clinical or messaging source (`Connected` without importing records).
    pub async fn connect_source(
        &self,
        source: SupportedSourceWire,
        body: &ConnectSourceBody,
    ) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut item = self.load_source(source)?;
        let label = if body.account_label.trim().is_empty() {
            format!("{} Demo Practice", source.label())
        } else {
            body.account_label.trim().to_owned()
        };
        if matches!(source, SupportedSourceWire::Hint)
            && matches!(body.method, SourceConnectionMethodWire::BrowserSession)
            && !body.prerequisites_confirmed
        {
            return Err(DbError::InvalidCommand(
                "Confirm the Chrome connector prerequisites before connecting Hint Health.".into(),
            ));
        }
        item.standing = SourceStandingWire::Connected {
            connected_at: DEMO_NOW.into(),
            method: body.method,
            account_label: label,
        };
        if source == SupportedSourceWire::Spruce
            && matches!(item.backlog, SpruceBacklogWire::NonePending)
        {
            item.backlog = SpruceBacklogWire::Offered {
                message_count: 12,
                earliest_message_at: "2026-07-15T14:00:00Z".into(),
            };
        }
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Records a Coda-assisted onboarding request on a practice source.
    pub async fn request_assisted_setup(
        &self,
        source: SupportedSourceWire,
        body: &RequestAssistedSetupBody,
    ) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let contact = body.contact_email.trim();
        if contact.is_empty() {
            return Err(DbError::InvalidCommand("Contact email is required.".into()));
        }
        let mut item = self.load_source(source)?;
        item.standing = SourceStandingWire::CodaAssistedRequested {
            requested_at: DEMO_NOW.into(),
            contact_email: contact.to_owned(),
            notes: body
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_owned),
        };
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Imports initial records for a connected practice source (`RecordsImported`).
    pub async fn import_source_records(
        &self,
        source: SupportedSourceWire,
    ) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut item = self.load_source(source)?;
        let (connected_at, method, account_label) = match &item.standing {
            SourceStandingWire::Connected {
                connected_at,
                method,
                account_label,
            }
            | SourceStandingWire::RecordsImported {
                connected_at,
                method,
                account_label,
                ..
            }
            | SourceStandingWire::UpToDate {
                connected_at,
                method,
                account_label,
                ..
            } => (connected_at.clone(), *method, account_label.clone()),
            _ => {
                return Err(DbError::Conflict(
                    "Connect this source before importing records.".into(),
                ));
            }
        };
        let count = item.retained_linked_records.max(18);
        item.retained_linked_records = count;
        item.standing = SourceStandingWire::RecordsImported {
            connected_at,
            method,
            account_label,
            imported_records: count,
            last_imported_at: DEMO_NOW.into(),
        };
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Refreshes a connected source cleanly into `UpToDate`.
    pub async fn refresh_source(
        &self,
        source: SupportedSourceWire,
    ) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut item = self.load_source(source)?;
        let (connected_at, method, account_label, imported_records) = match &item.standing {
            SourceStandingWire::Connected {
                connected_at,
                method,
                account_label,
            } => (connected_at.clone(), *method, account_label.clone(), 0),
            SourceStandingWire::RecordsImported {
                connected_at,
                method,
                account_label,
                imported_records,
                ..
            }
            | SourceStandingWire::UpToDate {
                connected_at,
                method,
                account_label,
                imported_records,
                ..
            }
            | SourceStandingWire::ReadFailed {
                connected_at,
                method,
                account_label,
                imported_records,
                ..
            } => (
                connected_at.clone(),
                *method,
                account_label.clone(),
                *imported_records,
            ),
            SourceStandingWire::AccessLost { .. } => {
                return Err(DbError::Conflict(
                    "Reconnect this source to restore authorization before refreshing.".into(),
                ));
            }
            _ => {
                return Err(DbError::Conflict(
                    "Connect this source before refreshing.".into(),
                ));
            }
        };
        item.standing = SourceStandingWire::UpToDate {
            connected_at,
            method,
            account_label,
            imported_records,
            last_read_at: DEMO_NOW.into(),
        };
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Accepts historical Spruce backlog review.
    pub async fn accept_spruce_backlog(&self) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut item = self.load_source(SupportedSourceWire::Spruce)?;
        let count = match item.backlog {
            SpruceBacklogWire::Offered { message_count, .. } => message_count,
            SpruceBacklogWire::Accepted {
                imported_message_count,
                ..
            } => imported_message_count,
            _ => 0,
        };
        item.backlog = SpruceBacklogWire::Accepted {
            accepted_at: DEMO_NOW.into(),
            imported_message_count: count,
        };
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Resolves a simulated Spruce release rollback halt back to `UpToDate`.
    pub async fn resolve_spruce_rollback(
        &self,
        _adopt_running_release: bool,
    ) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut item = self.load_source(SupportedSourceWire::Spruce)?;
        if let SourceStandingWire::RollbackHalted {
            connected_at,
            method,
            account_label,
            imported_records,
            ..
        } = item.standing
        {
            item.standing = SourceStandingWire::UpToDate {
                connected_at,
                method,
                account_label,
                imported_records,
                last_read_at: DEMO_NOW.into(),
            };
        }
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Reconnects a source from `AccessLost` while preserving saved clinical records.
    pub async fn reconnect_source(
        &self,
        source: SupportedSourceWire,
        body: &ConnectSourceBody,
    ) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut item = self.load_source(source)?;
        let (imported_records, label) = match &item.standing {
            SourceStandingWire::AccessLost {
                imported_records,
                account_label,
                ..
            } => (*imported_records, account_label.clone()),
            _ => (
                item.retained_linked_records,
                format!("{} Demo Practice", source.label()),
            ),
        };
        let account_label = if body.account_label.trim().is_empty() {
            label
        } else {
            body.account_label.trim().to_owned()
        };
        item.standing = SourceStandingWire::UpToDate {
            connected_at: DEMO_NOW.into(),
            method: body.method,
            account_label,
            imported_records,
            last_read_at: DEMO_NOW.into(),
        };
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Disconnects a practice source while retaining linked clinical records and reply drafts.
    pub async fn disconnect_source(
        &self,
        source: SupportedSourceWire,
    ) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut item = self.load_source(source)?;
        item.standing = SourceStandingWire::Disconnected;
        if source == SupportedSourceWire::Spruce {
            item.unlinked_spruce_messages = 0;
            item.backlog = SpruceBacklogWire::NonePending;
        }
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Simulates transient read failures or access loss on a source.
    pub async fn simulate_source_event(
        &self,
        source: SupportedSourceWire,
        body: &SimulateSourceEventBody,
    ) -> Result<SourceOperationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut item = self.load_source(source)?;
        let (connected_at, method, account_label, imported_records, last_good) =
            match &item.standing {
                SourceStandingWire::UpToDate {
                    connected_at,
                    method,
                    account_label,
                    imported_records,
                    last_read_at,
                } => (
                    connected_at.clone(),
                    *method,
                    account_label.clone(),
                    *imported_records,
                    Some(last_read_at.clone()),
                ),
                SourceStandingWire::RecordsImported {
                    connected_at,
                    method,
                    account_label,
                    imported_records,
                    last_imported_at,
                } => (
                    connected_at.clone(),
                    *method,
                    account_label.clone(),
                    *imported_records,
                    Some(last_imported_at.clone()),
                ),
                SourceStandingWire::ReadFailed {
                    connected_at,
                    method,
                    account_label,
                    imported_records,
                    last_successful_read_at,
                    ..
                }
                | SourceStandingWire::AccessLost {
                    connected_at,
                    method,
                    account_label,
                    imported_records,
                    last_successful_read_at,
                    ..
                } => (
                    connected_at.clone(),
                    *method,
                    account_label.clone(),
                    *imported_records,
                    last_successful_read_at.clone(),
                ),
                _ => (
                    DEMO_NOW.into(),
                    SourceConnectionMethodWire::SimulatedPractice,
                    format!("{} Demo Practice", source.label()),
                    item.retained_linked_records,
                    Some(DEMO_NOW.into()),
                ),
            };
        match body {
            SimulateSourceEventBody::QuietCatchUp
            | SimulateSourceEventBody::QuietTransientRetry { .. }
            | SimulateSourceEventBody::ClearQuietActivity => {}
            SimulateSourceEventBody::TransientReadFailure { reason } => {
                item.standing = SourceStandingWire::ReadFailed {
                    connected_at,
                    method,
                    account_label,
                    imported_records,
                    last_successful_read_at: last_good,
                    failed_at: DEMO_NOW.into(),
                    reason: reason.clone(),
                };
            }
            SimulateSourceEventBody::RevokedGrant { reason } => {
                item.standing = SourceStandingWire::AccessLost {
                    connected_at,
                    method,
                    account_label,
                    imported_records,
                    last_successful_read_at: last_good,
                    lost_at: DEMO_NOW.into(),
                    cause: AccessLossCauseWire::RevokedGrant,
                    reason: reason.clone(),
                };
            }
            SimulateSourceEventBody::AdministratorBlock { reason } => {
                item.standing = SourceStandingWire::AccessLost {
                    connected_at,
                    method,
                    account_label,
                    imported_records,
                    last_successful_read_at: last_good,
                    lost_at: DEMO_NOW.into(),
                    cause: AccessLossCauseWire::AdministratorBlock,
                    reason: reason.clone(),
                };
            }
        }
        self.save_source(&item)?;
        Ok(SourceOperationResponseWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            source: item,
        })
    }

    /// Assigns or removes a patient workspace on a physician's panel in SQLite.
    pub async fn assign_panel(
        &self,
        physician_id: &str,
        patient_workspace_id: &str,
        allowed: bool,
    ) -> Result<PanelAssignmentResponse, DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(patient_workspace_id)?;
        let key = format!("{physician_id}:{patient_workspace_id}");
        let insert = Query::insert()
            .into_table(PanelMembership::Table)
            .columns(PanelMembership::COLS.iter().copied())
            .values_panic([
                key.into(),
                physician_id.into(),
                patient_workspace_id.into(),
                if allowed { "1" } else { "0" }.into(),
            ])
            .on_conflict(
                OnConflict::column(PanelMembership::Key)
                    .update_column(PanelMembership::Allowed)
                    .to_owned(),
            )
            .to_owned();
        self.driver.insert(&insert)?;
        Ok(PanelAssignmentResponse {
            practice_id: DEMO_PRACTICE_ID.into(),
            physician_id: physician_id.to_owned(),
            patient_workspace_id: patient_workspace_id.to_owned(),
            allowed,
        })
    }

    /// Invites a roster patient to Duet and persists their invitation status in SQLite.
    pub async fn invite_patient(
        &self,
        request: &InvitePatientRequest,
    ) -> Result<PatientInvitationResponse, DbError> {
        self.ensure_ready().await?;
        let email = request.email.trim();
        if email.is_empty() {
            return Err(DbError::InvalidCommand(
                "Invitation email is required.".into(),
            ));
        }
        let resp = PatientInvitationResponse {
            practice_id: DEMO_PRACTICE_ID.into(),
            patient_workspace_id: request.patient_workspace_id.clone(),
            email: email.to_owned(),
            display_name: request.display_name.clone(),
            status: "invited".into(),
            token: Some(format!("demo-inv-{}", request.patient_workspace_id)),
            short_code: Some("DEMO-1234".into()),
            delivery_failed: false,
            detail: None,
        };
        self.update_patient_invitation_state(
            &request.patient_workspace_id,
            ConnectionStatusWire::Invited,
            Some(email.to_owned()),
            Some(&resp),
        )?;
        Ok(resp)
    }

    /// Re-sends an open patient Duet invitation.
    pub async fn resend_patient_invitation(
        &self,
        patient_workspace_id: &str,
    ) -> Result<PatientInvitationResponse, DbError> {
        self.ensure_ready().await?;
        let key = format!("patient:{patient_workspace_id}");
        let rows = self.driver.select_where(
            Invitations::Table,
            &[Invitations::PayloadJson],
            &[(Invitations::Key, key.as_str())],
        )?;
        let Some(row) = rows.first() else {
            return Err(not_found("patient invitation", patient_workspace_id));
        };
        Ok(serde_json::from_str(&row.get_str("payload_json")?)?)
    }

    /// Cancels an open patient Duet invitation and restores `NotInvited`.
    pub async fn cancel_patient_invitation(
        &self,
        patient_workspace_id: &str,
    ) -> Result<CancelInvitationResponse, DbError> {
        self.ensure_ready().await?;
        self.update_patient_invitation_state(
            patient_workspace_id,
            ConnectionStatusWire::NotInvited,
            None,
            None,
        )?;
        Ok(CancelInvitationResponse {
            practice_id: DEMO_PRACTICE_ID.into(),
            patient_workspace_id: patient_workspace_id.to_owned(),
            status: "not_invited".into(),
        })
    }

    /// Invites a colleague physician into the demo practice.
    pub async fn invite_colleague(
        &self,
        request: &InviteColleagueRequest,
    ) -> Result<ColleagueInvitationResponse, DbError> {
        self.ensure_ready().await?;
        let email = request.email.trim();
        let name = request.display_name.trim();
        if email.is_empty() || name.is_empty() {
            return Err(DbError::InvalidCommand(
                "Colleague email and display name are required.".into(),
            ));
        }
        let resp = ColleagueInvitationResponse {
            practice_id: DEMO_PRACTICE_ID.into(),
            email: email.to_owned(),
            display_name: name.to_owned(),
            role: "physician".into(),
            status: "invited".into(),
            delivery: "stored".into(),
            token: Some(format!("demo-colleague-{email}")),
            sign_in_path: None,
        };
        let key = format!("colleague:{email}");
        let insert = Query::insert()
            .into_table(Invitations::Table)
            .columns(Invitations::COLS.iter().copied())
            .values_panic([
                key.as_str().into(),
                "colleague".into(),
                "".into(),
                serde_json::to_string(&resp)?.as_str().into(),
            ])
            .on_conflict(
                OnConflict::column(Invitations::Key)
                    .update_column(Invitations::PayloadJson)
                    .to_owned(),
            )
            .to_owned();
        self.driver.insert(&insert)?;
        Ok(resp)
    }

    fn update_patient_invitation_state(
        &self,
        ws_id: &str,
        status: ConnectionStatusWire,
        invited_email: Option<String>,
        payload: Option<&PatientInvitationResponse>,
    ) -> Result<(), DbError> {
        let rows = self.driver.select_where(
            Patients::Table,
            &[Patients::WorkspaceJson],
            &[(Patients::WorkspaceId, ws_id)],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("patient workspace", ws_id))?;
        let mut ws: PatientWorkspaceResponse =
            serde_json::from_str(&row.get_str("workspace_json")?)?;
        ws.connection_status = status;
        let link = ws.patient_link.get_or_insert_with(|| PatientLinkWire {
            practice_id: DEMO_PRACTICE_ID.into(),
            patient_workspace_id: ws_id.to_owned(),
            duet_account_id: None,
            linked: false,
            sharing_consented: false,
            connection_status: status,
            consent_version: None,
            consented_at: None,
            revoked_at: None,
            invited_email: None,
            wrong_dob_attempts: 0,
            locked_at: None,
            delivery_failed_at: None,
            delivery_error: None,
        });
        link.connection_status = status;
        link.invited_email = invited_email;
        self.driver.update_where(
            Patients::Table,
            Patients::WorkspaceJson,
            serde_json::to_string(&ws)?.into(),
            &[(Patients::WorkspaceId, ws_id)],
        )?;
        let key = format!("patient:{ws_id}");
        if let Some(inv) = payload {
            let insert = Query::insert()
                .into_table(Invitations::Table)
                .columns(Invitations::COLS.iter().copied())
                .values_panic([
                    key.as_str().into(),
                    "patient".into(),
                    ws_id.into(),
                    serde_json::to_string(inv)?.as_str().into(),
                ])
                .on_conflict(
                    OnConflict::column(Invitations::Key)
                        .update_column(Invitations::PayloadJson)
                        .to_owned(),
                )
                .to_owned();
            self.driver.insert(&insert)?;
        } else {
            self.driver
                .delete_where(Invitations::Table, &[(Invitations::Key, key.as_str())])?;
        }
        Ok(())
    }

    fn load_source(&self, source: SupportedSourceWire) -> Result<PracticeSourceWire, DbError> {
        let rows = self.driver.select_where(
            PracticeConnections::Table,
            &[PracticeConnections::PayloadJson],
            &[(PracticeConnections::SourceId, source.slug())],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("practice connection", source.slug()))?;
        Ok(serde_json::from_str(&row.get_str("payload_json")?)?)
    }

    pub(crate) fn save_source(&self, source: &PracticeSourceWire) -> Result<(), DbError> {
        let insert = Query::insert()
            .into_table(PracticeConnections::Table)
            .columns(PracticeConnections::COLS.iter().copied())
            .values_panic([
                source.source_id.slug().into(),
                DEMO_PRACTICE_ID.into(),
                serde_json::to_string(source)?.as_str().into(),
            ])
            .on_conflict(
                OnConflict::column(PracticeConnections::SourceId)
                    .update_column(PracticeConnections::PayloadJson)
                    .to_owned(),
            )
            .to_owned();
        self.driver.insert(&insert)
    }
}
