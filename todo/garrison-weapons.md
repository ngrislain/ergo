# Garrison weapons rollout

This item is not published. The accepted 27,833-piece castle mountain garrison
(see `docs/model-recipes.md`) still has **300 unarmed plate-free figures**: each
holds nothing, because the plate-free conversion removed the legacy supported
weapons and the reviewed grip/shoulder/wrist estimates arrived after that
checkpoint.

## Goal

Give every one of the 300 accepted Lion Knights figures a weapon by replacing
its molded torso (`76382p4f`, arms and hands included) with the reviewed
`medieval-guard-v3` armed torso assembly, atomically per figure:

- `973p4f` printed torso (color 4, matching the existing base),
- reposed `3818`/`3819` arms (color 1) and `3820` hands (color 14) from the
  construct,
- upright `4497` spear (color 70) gripped in the right hand,
- `3846p4h` lion shield (color 4) held by the left hand.

Each figure keeps its own helmet variant (`3844`/`3896`/`89520`), cape,
subtle head turn, legs and every other piece ID. The parts adapt the
`medieval-guard-v3` construct (Stefan Frenz [smf], CCAL 2.0, official OMR
entry for set 1034).

## Survey results (2026-11-18, MCP read pass)

- All 300 figures are complete: legs + torso + head + helmet; ~200 also wear
  capes. No weapon/shield parts exist anywhere in the assembly.
- All torsos are axis-aligned; four yaw classes:
  identity 231, 180° 25, 90° 22, 270° 22 (wall guards face outward).
- Per-figure transform: `T = [R | p − R·(2, 1.5, 3)]` where `p` is the torso
  piece position and `R` its rotation; metric matrices equal yaw rotations and
  the construct's arm/hand rotations convert with the standard metric scaling.
- `place_submodel` `replace_ids` only accepts pieces loaded in the working
  view; each batch needs a `read_construction_page` load step first.

## Work state and blockers

- A single-figure test edit (courtyard figure, torso ID 14387) was attempted:
  the first try was rejected locally (`A replacement brick is absent`, target
  not loaded); the retry ended with `HTTP 502: Non-JSON response from
  publication API` and **no accepted receipt**, so that write is *uncertain*
  and must be reconciled before any retry.
- The shared publication API was unstable during the survey (connection resets
  and `HTTP 502: Generic S3 error` on plain reads). Publishing 300 atomic
  figure edits is blocked until that endpoint is healthy.

## Next steps

1. Reconcile the uncertain test write: read figure 14387 back at the opened
   revision and check whether its torso was replaced; record the outcome before
   any retry. Never blindly retry an uncertain shared write.
2. If the publication API is healthy, roll out in small self-supporting batches
   (load each batch's torso IDs, then one `place_submodel` per batch), watching
   each receipt.
3. After all 300 figures are armed: inspect the accepted receipt, capture
   actual renders, and record the accepted checkpoint in `docs/model-recipes.md`.
4. Optional follow-up: place `medieval-archer-v1` (genuine `4499` bow and
   arrow) for designated archer figures.
