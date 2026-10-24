# Progressive stud-specific level of detail

## What other viewers document

- **LDView** substitutes optimized/smooth primitives, offers low-resolution stud
  files, and has a separate curve-quality control. Its "Texture studs" option
  textures the LEGO logo; it is not evidence that it replaces whole studs with
  flat textures at a distance. It documents trilinear/anisotropic filtering for
  those textures. Source: [LDView help, Primitives](https://github.com/tcobbs/ldview/blob/master/Help.html#L2198-L2289).
- **LeoCAD** has multiple mesh LODs and treats stud/style primitives separately,
  including the `8/stud*.dat` family. Source: [mesh implementation](https://github.com/leozide/leocad/blob/master/common/lc_mesh.cpp)
  and [stud primitive handling](https://github.com/leozide/leocad/blob/master/common/lc_library.cpp).
- **BrickLink Studio** publicly documents rendering quality, logo effects,
  lighting and camera controls. Its Eyesight quality levels are sample counts
  for photoreal rendering, not a documented interactive distance-LOD algorithm.
  The available documentation does not establish its proprietary stud LOD
  thresholds. Source: [official Eyesight options](https://studiohelp.bricklink.com/hc/en-us/articles/6505772624023-Photoreal-Eyesight-options).
- **LDraw** defines geometry/primitives, not a mandatory rendering LOD policy.
  Its parts specification discusses normal/high-resolution primitives and
  precision: [official library specifications](https://www.ldraw.org/article/512.html).

Ergo adopts the useful separation of part bodies, stud primitives, and filtered
surface detail. It does not claim to reproduce Studio's internal renderer.

## Implemented transition

Previously body-size LOD could drop studs with a whole mesh, and the real ground
stud radius could exceed the generated grid, causing a hard ring edge. Studs now
have their own **projected CSS-pixel diameter**:

$$
p = \frac{H}{2\tan(\mathrm{fov}/2)}\frac{d}{r},\qquad
w = \operatorname{smoothstep}(1.25,6,p).
$$

`H` is viewport height; `d` is the transformed stud diameter; `r` is view distance.
For the standard studded catalog parts, positive-height stud geometry smoothly
approaches the body surface as `w` decreases. A derivative-filtered local-space
rim normal takes over with complementary weight. Known studded parts retain their
stud-capable mesh until the geometric transition reaches its flat endpoint,
instead of abruptly losing studs at the body-box threshold.

Ground studs use the same projected-size weight plus a wide distance taper
between 60% and 100% of the geometric radius. The radius is capped at 94 units,
inside a 193×193 candidate grid, so the grid boundary cannot cut off nonzero-height
studs. Near cylinders use 16 sides; small distant cylinders use 8. The far baseplate
shader retains filtered stud relief and fades that detail only as it becomes
subpixel. The cap body color is unchanged; this is not alpha-fading whole bricks.

The main and shadow passes use the same morphed geometry. Source meshes, saved
transforms, body bounds, picking and physics are unchanged. Full near detail keeps
nominal physical proportions. As with any LOD, the far image is an approximation.
Automatic analytic stud relief applies only to the known rectangular studded
catalog parts and virtual ground, not arbitrary specialty geometry or tiles.
Generic body/aggregate LOD still exists; this is not a guarantee of seamless
cross-fading of every nonstandard model feature.

Implementation: `crates/client/src/lod.rs`, `renderer.rs`,
`renderer/{shaders,illumination}.rs`, and `crates/world/src/geometry.rs`.
The Rust and GLSL transition constants must remain synchronized. Tests check
continuity/monotonicity, real WebGL main/shadow passes and browser navigation.
