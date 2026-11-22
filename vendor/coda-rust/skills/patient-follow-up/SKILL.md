---
name: patient-follow-up
description: Use when creating, reassessing, revising, completing, or ending a patient follow-up responsibility across chart updates, messages, and scheduled reassessments.
---

# Patient Follow-Up Reassessment

A follow-up is Coda's durable clinical responsibility to check one patient's chart and context
again on a planned date, so pending results, interval monitoring, specialist loops, medication
checks, and unanswered clinical questions are neither dropped nor surfaced prematurely.

## Authority and Safety Boundaries

- A follow-up authorizes future reassessment only. It never authorizes messaging the patient,
  placing orders, changing medications, scheduling appointments, or writing back to external
  systems.
- A follow-up is never a substitute for urgent or continuous clinical monitoring. Time-critical
  safety findings belong in an immediate patient concern with urgent (`now`) priority, never
  deferred to a future follow-up date.
- Keep each responsibility scoped to one clear clinical loop. Never alter, merge, or drop an
  unrelated follow-up on the same patient while reassessing the current item.

## Understanding the Responsibility Before Deciding

Before choosing a reassessment outcome, ground your review in:

1. What the current follow-up tracks, why it was created, its scheduled date, its constraints, and
   its prior update history.
2. Saved patient understanding, active patient memory entries (especially physician corrections or
   practice preferences), open concerns, and sibling active follow-ups.
3. Freshness of connected sources. Call `refresh_patient_sources` when newer laboratory, imaging,
   medication, or outside encounter records could resolve or change the responsibility, then search
   with `search_patient_information` and verify exact dated records with `read_patient_sources`,
   including individual document pages when needed.
4. Clinical reference details via `search_clinical_library` and `read_clinical_library` (using
   clinical concepts only, never patient identifiers) when surveillance intervals, diagnostic
   thresholds, or drug monitoring rules affect the reassessment.

## Choosing the Reassessment Outcome

Distinguish four clinical dispositions clearly and justify the choice with dated evidence and any
material limits:

- **Continue unchanged**: Keep the responsibility active on its current schedule when an expected
  study, consultation report, or response is still within a clinically reasonable window and no
  change in summary, timing, or physician attention is warranted. Quiet continuation is appropriate
  when nothing actionable has changed yet.
- **Change the next reassessment**: Update the summary, cited records, or next reassessment date
  when partial interval evidence arrives (for example, one component of a panel resulted while
  another remains pending), when a clinical plan shifts the monitoring window, or when an overdue
  check needs a revised next window alongside appropriate physician visibility.
- **Complete the responsibility**: Mark the follow-up complete only when verified dated evidence
  confirms that the clinical objective is satisfied (for example, the repeat potassium or imaging
  report has been read and reviewed, or the physician documented completion of the loop). Cite the
  confirming records and state what resolved it.
- **End without completion**: Cancel the responsibility when it is no longer clinically applicable—
  for example, the medication being monitored was discontinued before testing, care transferred
  permanently elsewhere, or a physician correction superseded the premise. State why the
  responsibility ended and never describe an ended or cancelled follow-up as clinically completed.
  A cancelled follow-up stays cancelled and must not be recreated unless genuinely new clinical
  information arrives that warrants watching for it again.

## Missing Evidence, Silence, Overdue Work, and Conflicting Records

- **Silence never implies resolution**: Absence of a patient reply, unreturned home readings, or a
  missing expected laboratory or specialist report never resolves a follow-up. Keep the uncertainty
  explicit and either continue within window, advance to a concrete reassessment date, or surface a
  concern when the delay itself carries clinical risk.
- **Overdue responsibilities**: Weigh the clinical stakes of what remains missing. Routine low-risk
  items slightly past their target window may continue with an updated reassessment date and clear
  rationale, whereas overdue high-consequence monitoring (such as abnormal test follow-up, post-ED
  recheck, or high-risk medication safety labs) warrants raising or updating a patient concern
  while keeping the follow-up state accurate.
- **Conflicting dated records**: When two sources disagree on dates, values, medication status, or
  whether a study occurred, cite both exact revisions, state the discrepancy plainly beside your
  conclusion, and avoid closing the responsibility as complete until the conflict is reconciled.
- **Unreadable or unavailable sources**: When a scanned page or attachment cannot be read or a
  connected source fails to refresh, state that limit explicitly rather than assuming normal
  findings.

## Longitudinal Coherence and Concerns

- Respect active patient memory corrections from the physician; when a correction changes the
  clinical premise or target threshold of the follow-up, reflect that in the reassessment decision.
- Check existing open concerns before raising a new concern. Never raise duplicate concerns on
  successive runs when the underlying evidence and clinical urgency have not materially changed;
  revise an existing concern only when new evidence changes its urgency or clinical picture, and
  retire it only when verified evidence resolves its clinical basis.
