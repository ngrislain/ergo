---
name: build
description: Finds existing LEGO sets by feature for inspiration, then builds or edits constructions in the shared Ergo world. Use for LEGO design, set references, brick construction, or Ergo editing.
---

# Build with Ergo

Use Ergo MCP tools, not hand-edited cache files. Google sign-in and token approval
belong to the human's browser; never ask for a token in chat or print one.
If setup is needed, direct the human to complete the browser authorization opened
by `ergo-mcp`, or run `ergo-mcp --login` before reconnecting the MCP server.

Before a new design, call `get_lego_models` with the subject or a `feature`
such as `arches`, `pitched_roofs`, `steering`, `suspension`, `gears`,
`modular_buildings`, or `trees`. These are inspiration candidates, not proof a
set contains the requested mechanism. Retrieve relevant `model_id` results,
inspect their submodels, and preserve source, author and license when adapting.
Catalogue inventories are not spatial models; never invent a reference match.

For an existing model, use `open_shared_construction`; for new placement, use
`get_user_position` and its ground-level transform. Resolve real pieces/colors
with `get_catalog`. Keep minifigure scale unless asked otherwise, use genuine
specialty parts where appropriate, and follow the tool's building guidance.

Check the edit allowance, placement feedback and server validation. Only a
confirmed `shared_commit` establishes a shared edit. Never automatically retry
an uncertain write. Report acceptance, remaining limitations and attribution.

Work as a harness: inspect → act → review → adjust. At meaningful milestones,
call `get_build_feedback` with the user's goal and `include_images:true`. Read the
measured observations and actionable suggestions, then inspect the actual images
for proportions, silhouette, openings, roof details, palette and requested
features. Distinguish heuristic suggestions from observed defects and physical
acceptance from visual quality. Make targeted improvements, preserve what works,
and incorporate human feedback. Do not run duplicate validation or dynamics after
every accepted edit; investigate specific failures instead.
