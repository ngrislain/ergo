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
Interactive color geometry morphs; shadow parts retain full source detail so
camera-driven LOD does not remove or flatten resident casters. Ground studs use
the same morphed instances in both passes. Source meshes, saved
transforms, body bounds, picking and physics are unchanged. Full near detail keeps
nominal physical proportions. As with any LOD, the far image is an approximation.
Automatic analytic stud relief applies only to the known rectangular studded
catalog parts and virtual ground, not arbitrary specialty geometry or tiles.
Body proxies apply to known rectangular parts; arbitrary imported silhouettes remain
canonical through the medium tiers. Subpixel aggregate LOD still exists; this is not a guarantee of seamless
cross-fading of every nonstandard model feature.

Implementation: `crates/client/src/lod.rs`, `scene.rs`, `scene/ground.rs`, `renderer.rs`,
`renderer/{shaders,illumination}.rs`, and `crates/world/src/geometry.rs`.
The GLSL transition constants are emitted from Rust at shader compilation. Tests check
continuity/monotonicity, real WebGL main/shadow passes and browser navigation.
