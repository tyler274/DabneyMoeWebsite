//! Typed SQLite schema definitions built with `sea-query`.

use sea_query::{ColumnDef, Iden, Index, SqliteQueryBuilder, Table};

macro_rules! define_tables {
    ($($name:ident { $($col:ident),+ });+ $(;)?) => {
        $(
            #[derive(Iden, Clone, Copy)]
            pub(crate) enum $name { Table, $($col),+ }
            impl $name { pub(crate) const COLS: &'static [Self] = &[$(Self::$col),+]; }
        )+
    };
}

define_tables!(
    Patients { WorkspaceId, DisplayName, Mrn, BirthDate, WorkspaceJson };
    SourceRecords {
        Id,
        WorkspaceId,
        RecordGroup,
        Title,
        RecordedAt,
        Body,
        SourceLabel,
        ReadablePartsJson,
        FieldsJson,
        RecordStatus,
        RawContentJson
    };
    TodayItems { ItemId, WorkspaceId, Urgency, WaitingSince, Kind, RowJson, Done, Seen };
    PracticeVisits { Id, StartsAt, PayloadJson };
    UpdatesFeed { Id, WorkspaceId, Bucket, CreatedAt, PayloadJson };
    Chats { ChatId, WorkspaceId, Title, CreatedAt, SnapshotJson };
    Drafts { WorkspaceId, ChatId, Revision, Text };
    Briefings { BriefingId, WorkspaceId, PayloadJson };
    FollowUps { FollowUpId, WorkspaceId, Standing, UpdatedAt, PayloadJson };
    SpruceReplies { ConversationId, WorkspaceId, WaitingSince, PayloadJson };
    SpruceMatchQueue { MessageId, ConversationId, Bucket, WaitingSince, PayloadJson };
    PatientReviews { ReviewId, WorkspaceId, UpdatedAt, PayloadJson };
    PracticeConnections { SourceId, PracticeId, PayloadJson };
    PatientMemory { WorkspaceId, NoteSeq, PayloadJson };
    PanelMembership { Key, PhysicianId, WorkspaceId, Allowed };
    Invitations { Key, Kind, WorkspaceId, PayloadJson };
);

fn col_text<C: Iden + 'static>(col: C, pk: bool) -> ColumnDef {
    let mut def = ColumnDef::new(col);
    def.text().not_null();
    if pk {
        def.primary_key();
    }
    def
}

fn col_int<C: Iden + 'static>(col: C) -> ColumnDef {
    let mut def = ColumnDef::new(col);
    def.big_integer().not_null();
    def
}

fn text_table<T: Iden + 'static, C: Iden + Copy + 'static>(table: T, cols: &[C]) -> String {
    let mut stmt = Table::create();
    stmt.table(table).if_not_exists();
    for (i, c) in cols.iter().enumerate() {
        stmt.col(col_text(*c, i == 0));
    }
    stmt.build(SqliteQueryBuilder)
}

/// Returns the ordered DDL statements needed to initialize a fresh demo database.
pub fn schema_ddl() -> Vec<String> {
    let idx = Index::create()
        .if_not_exists()
        .name("idx_source_records_workspace")
        .table(SourceRecords::Table)
        .col(SourceRecords::WorkspaceId)
        .col(SourceRecords::RecordedAt)
        .build(SqliteQueryBuilder);

    let mut today = Table::create();
    today.table(TodayItems::Table).if_not_exists();
    for (i, c) in TodayItems::COLS[..6].iter().enumerate() {
        today.col(col_text(*c, i == 0));
    }
    today.col(col_int(TodayItems::Done));
    today.col(col_int(TodayItems::Seen));

    let mut drafts = Table::create();
    drafts
        .table(Drafts::Table)
        .if_not_exists()
        .col(col_text(Drafts::WorkspaceId, false))
        .col(col_text(Drafts::ChatId, false))
        .col(col_int(Drafts::Revision))
        .col(col_text(Drafts::Text, false))
        .primary_key(Index::create().col(Drafts::WorkspaceId).col(Drafts::ChatId));

    let mut memory = Table::create();
    memory
        .table(PatientMemory::Table)
        .if_not_exists()
        .col(col_text(PatientMemory::WorkspaceId, true))
        .col(col_int(PatientMemory::NoteSeq))
        .col(col_text(PatientMemory::PayloadJson, false));

    vec![
        text_table(Patients::Table, Patients::COLS),
        text_table(SourceRecords::Table, SourceRecords::COLS),
        idx,
        today.build(SqliteQueryBuilder),
        text_table(PracticeVisits::Table, PracticeVisits::COLS),
        text_table(UpdatesFeed::Table, UpdatesFeed::COLS),
        text_table(Chats::Table, Chats::COLS),
        drafts.build(SqliteQueryBuilder),
        text_table(Briefings::Table, Briefings::COLS),
        text_table(FollowUps::Table, FollowUps::COLS),
        text_table(SpruceReplies::Table, SpruceReplies::COLS),
        text_table(SpruceMatchQueue::Table, SpruceMatchQueue::COLS),
        text_table(PatientReviews::Table, PatientReviews::COLS),
        text_table(PracticeConnections::Table, PracticeConnections::COLS),
        memory.build(SqliteQueryBuilder),
        text_table(PanelMembership::Table, PanelMembership::COLS),
        text_table(Invitations::Table, Invitations::COLS),
    ]
}
