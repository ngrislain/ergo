# Nested authoring groups

## Persistent hierarchy, world-space pieces

A group contains pieces and/or other groups. The hierarchy is a forest: a member
has one direct parent, references must exist, cycles are rejected, and nesting is
bounded to 64 levels. Empty containers are allowed while authoring.

Each group stores an ID, name, member references, and world pivot. Piece poses
remain world-space and authoritative for spatial indexing, collision checks, and
physics. Transforming a container updates every descendant piece **once** and
also updates descendant pivots. Readers do not apply a second hidden parent
matrix to the already-transformed poses.

Group metadata is stored in paged shared rules and native construction exports.
Membership-only edits reuse unchanged geometry. Reusable model definitions and
authoring groups are distinct: source hierarchy is retained when its instances
match the authoritative piece poses.

## Rigid transforms and dimensions

Only translation/rotation is supported; catalog pieces are not scaled or sheared.
With public construction-to-SI scale $D=\operatorname{diag}(0.008,0.008,0.0096)$,
SI rotation $R$, world pivot $\mathbf p$, and construction-unit translation
$\mathbf t$:

$$
\mathbf x'=\mathbf p+D^{-1}RD(\mathbf x-\mathbf p)+\mathbf t,
\qquad A'=D^{-1}RD A.
$$

This matters for rotations mixing height with horizontal axes: a unit brick is
9.6 mm high and one stud pitch is 8 mm. Rotating raw construction coordinates as
if they were isotropic would distort real dimensions.

The rotation axis uses orthonormal ENU directions; angle is radians and
right-handed. Internal joint anchors/axes move consistently with the group.
An anchored member or a mechanical constraint crossing the group boundary blocks
the edit. Include the connected pieces or use physical constraint manipulation.

## Authoring versus physics

Grouping organizes transforms. It does not create stud bonds, weld bodies, or
make a collection survive gravity/earthquakes. Physical joints and contacts remain
the authority for attachment. A grouped assembly can fall apart in simulation.

Group transforms are atomic CAD edits. They validate the **destination** body
boxes and floor. A rotation is not a continuously swept physics trajectory;
conservative world AABBs may reject some otherwise valid rotated arrangements.
Dynamic dragging is handled by the separate client constraint solver.

## UI and MCP

The inspector can add piece IDs to a group-selection buffer. **Groups…** opens
creation, nested membership/reparenting, focus/highlight, translation, Z rotation,
and ungrouping controls. The interactive edit working set must be complete.

MCP tools are `list_groups`, `create_group`, `set_group_members`,
`transform_group`, and `ungroup`. For example:

```json
{"name":"Tower","piece_ids":[1,2,3]}
```

```json
{"name":"Building","group_ids":[1],"piece_ids":[4,5]}
```

```json
{
  "group_id":2,
  "translation":[4,0,0],
  "rotation_axis":[0,0,1],
  "rotation_radians":1.5707963267948966
}
```

Ungrouping preserves world poses and splices children into an existing parent.
Removing pieces also removes their direct membership references. A metadata edit
does not make an otherwise invalid draft publishable; the server checks the
materialized physical scene before a snapshot is committed.
