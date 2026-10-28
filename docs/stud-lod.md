# Progressive stud-specific level of detail

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

The passes share one stud-deformation helper and Rust-generated thresholds.
Interactive color geometry morphs. Auto/Performance shadow parts have an
independent **light-buffer** LOD: source, reviewed medium mesh or far mesh at
6/1.25-texel projected stud thresholds, with hysteresis. Camera-color clipping
does not remove resident offscreen casters. Unreviewed/source-only, selected and highlighted
shadow pieces keep source geometry; High, capture and illustration policies keep
all shadow parts at source detail. Ground studs use the same morphed instances
in both passes. The global color-budget bias does not replace the actual stud
pixel size: a standard studded part retains stud-capable geometry until that
transition reaches its flat endpoint. Source meshes, saved
transforms, body bounds, picking and physics are unchanged. Full near detail keeps
nominal physical proportions. As with any LOD, the far image is an approximation.
Automatic analytic stud relief applies only to the known rectangular studded
catalog parts and virtual ground, not arbitrary specialty geometry or tiles.
Body boxes apply to known rectangular parts; reviewed window-frame and round-brick
proxies retain their open/round silhouettes. Arbitrary imported silhouettes remain
canonical through the medium tiers. Subpixel aggregate LOD still exists; this is not a guarantee of seamless
cross-fading of every nonstandard model feature.

Implementation: `crates/client/src/lod.rs`, `scene.rs`, `scene/ground.rs`, `renderer.rs`,
`renderer/{shaders,illumination}.rs`, and `crates/world/src/geometry.rs`.
The GLSL transition constants are emitted from Rust at shader compilation. Tests check
continuity/monotonicity, real WebGL main/shadow passes and browser navigation.
