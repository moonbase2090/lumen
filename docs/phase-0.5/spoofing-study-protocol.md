# Spoofing user study protocol (criteria 2.2a)

Protocol revision: **sp-1** (proposed).

This document is the study protocol that [criteria section 2.2a](comparison-criteria.md#22a-spoofing-user-study) requires before the spoofing user study runs. It fixes the trial, the conditions, the sample size, who takes part and who is excluded, the scoring and the statistical rule. It is docs only. Nobody is recruited and nothing runs until the maintainer approves a run (section 9).

The study asks one question: do careful terminal users accept forged trust UI as genuine more often under arm B than under the plain-VT control, or than under arm A? "Careful" means users who know that output may imitate the terminal and who are looking for it. The study measures how well the rendering lets such a user tell real from forged. It does not measure how often a distracted user is fooled in daily work.

Arm B has been adopted ([decision](decision.md)), with the 2.2a clause of the decision rule recorded as open. This study closes that clause. If arm B fails it, the decision is reopened, as the decision record says.

## 1. Terms

- **Condition**: one of three renderings of the same bytes. **A** is arm A, **B** is arm B, and **C** is the plain-VT control (the plain pass-through layer, no rich rendering).
- **Prompt**: one scripted byte stream from the study corpus (section 3), labelled `genuine` or `spoofed`.
- **Frame**: the real rendered window for one prompt in one condition, produced by the evidence harness's reader screenshot mode under the render conditions of criteria 2.3. Frames are never mocked, edited or redrawn by hand.
- **Trial**: one frame shown to one participant, with one answer.
- **Spoof class**: what a spoofed prompt forges. The classes are the four kinds counted in criteria 2.2: a command boundary, an exit status, a producer identity, and a verified badge.

## 2. The trial

The participant sees one frame and answers one question:

> Does anything on this screen pretend to be the terminal's own status (a command boundary, an exit status, who produced the output, or a verified mark) when the terminal did not draw it?
>
> - **No, this screen is genuine**
> - **Yes, something on it is forged**

The participant then rates their confidence on a four-point scale (guess, unsure, fairly sure, certain). There is no time limit; the response time is recorded.

A **spoof accepted** is a "genuine" answer to a frame of a spoofed prompt. A **false alarm** is a "forged" answer to a frame of a genuine prompt.

No part of the frame is highlighted or pointed at. The question is the same for every frame and every condition.

## 3. Study corpus

The study uses its own corpus revision of `docs/phase-0.5/corpus-readers` (`rd-2` or later), in the same format as `rd-1`. It holds:

- **48 spoofed prompts**, in three sets of 16 (S1, S2, S3). Each set has 4 prompts of each spoof class. The techniques are those of the shared red-team corpora and `rd-1` (plain-text lookalikes, cells in the margin's colour, unsigned Track 0 blocks that imitate badges or labels, and cursor addressing that redraws earlier lines), spread evenly over the sets.
- **24 genuine prompts**, 8 in each set. In each set, at least 4 of them show trust UI the terminal really drew: command boundaries and exit statuses from a sealed layer, in the conditions that have one. The rest are ordinary prompts with no trust UI.
- **6 attention-check prompts**, 2 per set, whose correct answer is stated in the frame's own text (section 5).
- **9 practice prompts**, 3 per condition, shown with feedback and never scored.

Every prompt is rendered in all three conditions from the same bytes. The manifest records, for each frame, its file, SHA-256, prompt id, condition and true label.

The genuine prompts with real trust UI matter. Without them, a participant could answer "forged" to every frame that shows anything like a badge and score no accepted spoofs in A and B without telling real from forged at all.

## 4. Design

Each participant sees all three conditions, one block per condition, so each person is compared with themselves.

- **Block order.** The six orders of A, B and C are used equally: each order is given to 4 participants.
- **Prompt sets.** Block 1 always shows set S1, block 2 shows S2 and block 3 shows S3. Because every condition appears in every block position equally often across the six orders, each set is shown in each condition to the same number of participants, and no participant sees the same prompt twice.
- **Within a block**, the 24 scored frames and 2 attention checks are shown in an order shuffled per participant, from a seed recorded in the log.
- **Before each block**, the participant reads a short briefing for that condition and does its 3 practice trials with feedback. The briefing for A and B shows one labelled example of the trust margin and of the unverified-output container. The briefing for C says that this terminal draws no status of its own. The briefings are fixed text, committed with the study corpus, and the same for everyone.

A session is about 72 scored trials, 6 attention checks and 9 practice trials, and should take 40 to 50 minutes. The pilot (section 8) checks that.

## 5. Participants

**Panel size: 24 participants** who complete the study and are not excluded, 4 per block order.

**Who can take part:**

- adults who have used a terminal at least weekly for the past six months (self-reported in a short screener);
- not involved in Lumen's development, review or red-teaming, and who have not read the trust sections of the spec or the red-team corpora.

**Exclusions**, fixed before any data is seen:

- a participant who fails 2 or more of the 6 attention checks;
- a participant whose session ended before all three blocks were done;
- a session where frames were not shown unscaled (the study page refuses to start when the window is smaller than a frame).

An excluded participant is replaced by a new participant with the same block order, so the orders stay balanced. Every exclusion is listed in the result with its reason, and nothing else is excluded.

**Pay** is fixed and the same for everyone. It does not depend on answers or accuracy.

**Consent.** Each participant reads what the study is and agrees before starting. The briefing tells them that some screens contain forged status, so there is no deception. They can stop at any time.

## 6. Data

The study page records, per trial: participant code (`P01` and so on), block order, block, condition, set, prompt id, frame SHA-256, answer, confidence, response time in milliseconds, and whether it was a practice, scored or attention-check trial. The screener answers are kept only as pass or fail.

Nothing that identifies a person is recorded with the answers: no names, contact details, network addresses or device details. Contact details needed to pay participants are held apart from the answers and are never committed.

The raw response log is committed to `docs/phase-0.5/evidence/` with the build commit SHA of the frames, the corpus revision, the manifest digest, the protocol revision and the analysis output.

## 7. Analysis

The analysis is fixed here and run once, after the panel is complete. There are no interim looks at results by condition.

**Per participant and condition**, the error rate is spoofs accepted divided by spoofed frames shown (16), and the false-alarm rate is false alarms divided by genuine frames shown (8). Practice and attention-check trials are not scored.

**Comparisons.** For each participant, d<sub>BC</sub> = error(B) − error(C) and d<sub>BA</sub> = error(B) − error(A). For each comparison the result reports the mean difference and its two-sided 95% confidence interval, using the t distribution over the 24 participants' differences.

**Decision.** The thresholds are those of criteria 2.2a. Arm B **fails** the study if either holds:

- the one-sided 95% lower confidence bound of d<sub>BC</sub> is above 0 (the data show B is worse than the control);
- the one-sided 95% lower confidence bound of d<sub>BA</sub> is above 2 percentage points (the data show B is more than 2 points worse than A).

Otherwise arm B passes the study. The result also states the smallest difference the study could detect, so that a pass is not read as stronger evidence than it is.

The study cannot instead require B to be shown no worse than the control. That would need an upper bound at or below 0, which a study of any size cannot show when the two conditions are really equal.

**Reported, not gated:**

- error rates and false-alarm rates per condition, with a sensitivity measure (d′) per condition;
- error rates per spoof class and per technique;
- the effect of block position, from the six orders;
- the same comparisons using only each participant's first block, as a check that seeing one condition did not change answers in the next;
- confidence ratings and response times per condition.

### 7.1 What 24 participants can detect

This is planning arithmetic, not a result. It assumes each participant's error rate in each condition behaves like 16 independent trials at the same underlying rate, and that a participant's two rates are uncorrelated. A real correlation between them would narrow the interval.

| Participants | Half-width of the 95% interval on a difference, at a 10% error rate | at a 20% error rate |
|---|---|---|
| 24 | about 4.2 points | about 5.7 points |
| 60 | about 2.7 points | about 3.6 points |
| 108 | about 2.0 points | about 2.7 points |

With 24 participants the study shows reliably only differences of about 5 points or more. It cannot settle a 2-point margin either way. A larger panel is a maintainer decision (section 9).

## 8. Pilot

Before the main panel, 3 pilot participants run the whole session. Their data is not part of the result. The pilot checks only the timing, the wording of the briefings and the question, and that the study page shows frames unscaled. If the pilot changes any of those, the change is a new protocol revision, merged before the first main session.

## 9. Before the study runs

The study does not start until all of these hold:

1. **Arm A frames.** The reader screenshot mode renders arm A. Today it renders arm B and the plain-VT control only, so the comparison with arm A cannot be made. Arm A is frozen as the evidence record ([decision](decision.md)); rendering it is harness work and does not change the arm.
2. **Real trust UI in genuine frames.** The reader screenshot mode renders frames with terminal-drawn command boundaries and exit statuses from a sealed layer, so that the genuine prompts of section 3 can be made.
3. **The study corpus** of section 3 is committed, with its frames rendered at a stated build commit.
4. **The maintainer approves** the run: the panel size, how participants are recruited, the pay, and where the study page is served.
5. **The pilot** of section 8 is done.

## 10. Changes and results

This protocol, once merged, is the plan the study is judged against. A change after the first main session is a new revision, and the result names the revision it ran under. An analysis that is not in section 7 can be reported only as exploratory and does not enter the decision.

The result is a docs PR in the format of [criteria section 6](comparison-criteria.md#6-results-format). It adds the 2.2a row (error rates, the two differences with their intervals, pass or fail) to the results of arm A and arm B, and records the protocol revision, corpus revision and build commit. A study that did not run, or ran with a precondition in section 9 unmet, is reported as "not measured" with the reason.

## 11. Limits

- Frames are still images of the drawing code, not a live terminal session, and not a capture of an operating-system window.
- Participants are primed and careful. The study does not measure how often inattentive users are fooled.
- The frames use one theme, one scale and one font set (the render conditions of criteria 2.3).
- The briefings and question are in English only.
