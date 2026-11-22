---
name: patient-memory-upkeep
description: Use when you change a patient's memory pages, summary or direction, in a conversation or during patient rounds.
---

# Patient Memory Upkeep

Memory is the picture of this patient that later work starts from. Keep it true, short at the top
and easy to search.

## Do the reading

- In patient rounds, read every log day listed at the start, whole. The notes shown there can be
  cut short.
- Read a file before you change it.
- Before you write that the chart shows something, look at the chart. A note that names a record
  is not a reading of it.

## What belongs in memory

- What was worked out, said or decided.
- Dated history that takes several records to put together.
- Why a medication was started, changed or stopped.
- Not the chart. Current medications, doses and values stay in the chart. Do not copy a record
  into memory.

## Pages

- One page for one subject: a problem, a symptom not yet explained, an episode, prevention.
- The person page, person.md, holds the person and their people: goals, preferences, home,
  family, care team.
- Search before you make a page. Add to the page that exists.
- An earlier run may have stopped part way. Search for a fact before you add it, so it is not
  written twice.
- A page is markdown. Start it with its name, the other names people use for it, and your word
  for its kind. The other names are how a search finds the page. Then the facts. For example:

  ```
  ---
  name: Hypertension
  other names: [high blood pressure, HTN]
  kind: condition
  ---

  ## History

  - 2017-09: BP about 150/88 on HCTZ taken unevenly; lisinopril added. [chart]
  - 2025-06-20: The physician said to hold lisinopril until the cough clears. [N4]
  ```

- Two pages on the same thing: move the facts to one and delete the other.

## Facts

- One fact to a line.
- Start a dated fact with its date, as a calendar date: "2025-06-20: ...". Never "yesterday" or
  "last week".
- Do not guess a date. A fact with no known date says so.
- Say in the words where a fact came from when that matters: "the physician said", "the ED note
  says".
- A short tag can say what a fact rests on: [N4] for a note, [chart] for the chart. Many facts
  need none.
- Write a fact that will still be true next month.
- Link to another page with [[page-id]] where a reader would follow it.

## Keeping it clean

- A fact that is now wrong: replace the wrong words. Do not leave both.
- Remove what is stale, repeated or no longer matters.
- Fix or drop a link to a page that does not exist.
- Change the part that needs it. Do not clear a page and write it again.

## Direction

- Add a direction only when the physician meant it to hold from now on.
- A request for one answer is not a direction.
- Write it in plain words, close to the physician's own. Use the shortest wording that keeps
  their meaning. Add nothing they did not say.
- A new direction that replaces an older one: replace the older one.

## Summary

- The patient in one line, what matters now, what is open, then the pages.
- Keep it short. Move detail to a page and leave a link.
- The direction loads beside the summary. Do not repeat it.
- Each edit tells you the file's size.

## When the physician and the chart do not agree

- The physician told Coda something the chart does not hold: keep it as what the physician said.
- A note says the chart shows something, and the chart does not: leave that off the page. The note
  stays in the log.
- The physician and the chart disagree: state both, each with its date.

## When a write reports a problem

- An edit that could not apply is named in the result. The rest are saved.
- Fix what was left out if it still matters.

## When nothing is new

- Change no file, and say so.
- New notes with nothing worth a line: say which notes you read and why none earned one.
