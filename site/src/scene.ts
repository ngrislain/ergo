import {
  AmbientLight,
  Color,
  CylinderGeometry,
  DirectionalLight,
  Group,
  HemisphereLight,
  InstancedMesh,
  Matrix4,
  Mesh,
  MeshStandardMaterial,
  OrthographicCamera,
  PCFSoftShadowMap,
  PlaneGeometry,
  Quaternion,
  Scene,
  ShadowMaterial,
  SphereGeometry,
  SRGBColorSpace,
  Vector3,
  WebGLRenderer,
} from "three";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";
import type { BufferGeometry, Material } from "three";

const palette = {
  blue: "#315fed",
  yellow: "#ffd23f",
  red: "#ee6748",
  mint: "#a3c6ac",
  paper: "#fffdf5",
  base: "#ccd8ea",
  brown: "#a77a58",
  ink: "#182a3a",
};
type ColorName = keyof typeof palette;
type Piece = {
  group: Group;
  rest: Vector3;
  restRotation: Quaternion;
  from: Vector3;
  to: Vector3;
  fromRotation: Quaternion;
  toRotation: Quaternion;
  start: number;
  duration: number;
};

export function mountBrickScene(mount: HTMLElement): () => void {
  const status = document.querySelector<HTMLElement>("[data-scene-status]");
  const buttons = [
    ...document.querySelectorAll<HTMLButtonElement>("[data-scene-action]"),
  ];
  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let renderer: WebGLRenderer;
  try {
    renderer = new WebGLRenderer({
      antialias: true,
      alpha: true,
      powerPreference: "low-power",
    });
  } catch {
    mount.classList.add("scene-unavailable");
    if (status) status.textContent = "The guide is ready, even without WebGL.";
    return () => {};
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 1.75));
  renderer.outputColorSpace = SRGBColorSpace;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = PCFSoftShadowMap;
  const canvas = renderer.domElement;
  canvas.setAttribute(
    "aria-label",
    "An interactive brick-built doorway with a little builder and a tree. Use the buttons to rotate, take it apart, or add a brick.",
  );
  canvas.setAttribute("role", "img");
  canvas.style.touchAction = "pan-y";
  mount.append(canvas);
  mount.classList.add("scene-ready");

  const scene = new Scene();
  const camera = new OrthographicCamera(-12, 12, 10, -10, 0.1, 150);
  camera.position.set(20, 18, 25);
  camera.lookAt(0, 3.2, 0);
  scene.add(new HemisphereLight("#ffffff", "#b4bdd0", 2.7));
  scene.add(new AmbientLight("#ffffff", 0.7));
  const light = new DirectionalLight("#fff5df", 3.7);
  light.position.set(-10, 18, 12);
  light.castShadow = true;
  light.shadow.mapSize.set(2048, 2048);
  Object.assign(light.shadow.camera, {
    left: -18,
    right: 18,
    top: 18,
    bottom: -18,
    near: 0.1,
    far: 60,
  });
  light.shadow.normalBias = 0.035;
  light.shadow.bias = -0.0002;
  scene.add(light);

  const root = new Group();
  scene.add(root);
  root.rotation.y = -0.12;
  let targetYaw = root.rotation.y;
  let hoverYaw = 0;
  let hoverPitch = 0;
  let scrollProgress = 0;
  const geometries = new Map<string, BufferGeometry>();
  const materials = new Map<ColorName, MeshStandardMaterial>();
  const extraMaterials: Material[] = [];
  const studGeometry = new CylinderGeometry(0.255, 0.275, 0.17, 20);
  geometries.set("stud", studGeometry);
  const eyeGeometry = new SphereGeometry(0.11, 12, 8);
  geometries.set("eye", eyeGeometry);
  const pieces: Piece[] = [];
  const matrix = new Matrix4();
  function material(color: ColorName) {
    if (!materials.has(color))
      materials.set(
        color,
        new MeshStandardMaterial({
          color: new Color(palette[color]),
          roughness: 0.34,
          metalness: 0.02,
        }),
      );
    return materials.get(color)!;
  }
  function brick(
    width: number,
    depth: number,
    color: ColorName,
    x: number,
    y: number,
    z: number,
    rotation = 0,
    height = 0.96,
  ) {
    const key = `${width}/${depth}/${height}`;
    if (!geometries.has(key))
      geometries.set(
        key,
        new RoundedBoxGeometry(width - 0.045, height, depth - 0.045, 3, 0.075),
      );
    const group = new Group();
    group.position.set(x, y, z);
    group.rotation.y = rotation;
    const body = new Mesh(geometries.get(key), material(color));
    body.castShadow = true;
    body.receiveShadow = true;
    group.add(body);
    const studs = new InstancedMesh(
      studGeometry,
      material(color),
      width * depth,
    );
    let index = 0;
    for (let i = 0; i < width; i++) {
      for (let j = 0; j < depth; j++) {
        matrix.makeTranslation(
          i - (width - 1) / 2,
          height / 2 + 0.075,
          j - (depth - 1) / 2,
        );
        studs.setMatrixAt(index++, matrix);
      }
    }
    studs.instanceMatrix.needsUpdate = true;
    studs.computeBoundingSphere();
    studs.castShadow = true;
    studs.receiveShadow = true;
    group.add(studs);
    root.add(group);
    const piece: Piece = {
      group,
      rest: group.position.clone(),
      restRotation: group.quaternion.clone(),
      from: group.position.clone(),
      to: group.position.clone(),
      fromRotation: group.quaternion.clone(),
      toRotation: group.quaternion.clone(),
      start: 0,
      duration: 0,
    };
    pieces.push(piece);
    return piece;
  }

  // Every visible body and stud is geometry: no image or model downloads.
  brick(18, 12, "base", 0, -0.3, 0, 0, 0.36);
  brick(6, 2, "paper", 0, 0.38, 3.3, 0, 0.48);
  brick(4, 2, "blue", 0, 0.62, 1.3);
  for (let layer = 0; layer < 6; layer++) {
    brick(2, 2, layer === 0 ? "paper" : "red", -3.5, 0.62 + layer * 1.06, -0.8);
    brick(2, 2, layer === 0 ? "paper" : "blue", 3.5, 0.62 + layer * 1.06, -0.8);
  }
  brick(2, 4, "paper", -3.5, 6.98, -0.8);
  brick(2, 4, "paper", 3.5, 6.98, -0.8);
  brick(10, 2, "yellow", 0, 8.04, -0.8);
  brick(4, 2, "yellow", -2, 9.1, -0.8);
  brick(4, 2, "yellow", 2, 9.1, -0.8);
  brick(4, 2, "red", 0, 10.16, -0.8);
  for (let layer = 0; layer < 3; layer++)
    brick(1, 1, "brown", -6.4, 0.62 + layer * 1.06, -3.6);
  brick(2, 2, "mint", -6.4, 3.8, -3.6);
  brick(4, 2, "mint", -6.4, 4.86, -3.6);
  brick(2, 2, "mint", -6.4, 5.92, -3.6);
  // A small builder gives the architectural illustration its scale.
  brick(1, 2, "blue", 5.5, 0.62, 3.1);
  brick(1, 2, "blue", 6.65, 0.62, 3.1);
  brick(2, 2, "yellow", 6.08, 1.68, 3.1);
  const head = brick(2, 2, "yellow", 6.08, 2.74, 3.1);
  for (const x of [-0.43, 0.43]) {
    const eye = new Mesh(eyeGeometry, material("ink"));
    eye.position.set(x, 0.02, 0.99);
    head.group.add(eye);
  }
  brick(2, 2, "red", 6.08, 3.8, 3.1, 0, 0.45);
  brick(1, 1, "yellow", 4.65, 1.68, 3.1);
  brick(1, 1, "yellow", 7.5, 1.68, 3.1);
  brick(2, 4, "blue", -5, 0.62, 3.8, -0.22);
  brick(2, 2, "red", -7.15, 0.62, 0.2, 0.3);
  brick(2, 3, "paper", 5.7, 0.62, -3.7, -0.13);

  const floorGeometry = new PlaneGeometry(100, 100);
  geometries.set("floor", floorGeometry);
  const floorMaterial = new ShadowMaterial({ opacity: 0.13 });
  extraMaterials.push(floorMaterial);
  const floor = new Mesh(floorGeometry, floorMaterial);
  floor.rotation.x = -Math.PI / 2;
  floor.position.y = -0.49;
  floor.receiveShadow = true;
  scene.add(floor);

  let disposed = false;
  let visible = true;
  let frame = 0;
  let exploded = false;
  let added = 0;
  const eventAbort = new AbortController();
  function requestRender() {
    if (!frame && !disposed && visible && !document.hidden)
      frame = requestAnimationFrame(render);
  }
  function render(now: number) {
    frame = 0;
    if (disposed || !visible || document.hidden) return;
    let moving = false;
    for (const piece of pieces) {
      if (!piece.duration) continue;
      const t = Math.max(0, Math.min(1, (now - piece.start) / piece.duration));
      const ease = 1 - Math.pow(1 - t, 4);
      piece.group.position.lerpVectors(piece.from, piece.to, ease);
      piece.group.quaternion.slerpQuaternions(
        piece.fromRotation,
        piece.toRotation,
        ease,
      );
      if (t < 1) moving = true;
      else piece.duration = 0;
    }
    const viewYaw = targetYaw + hoverYaw + scrollProgress * 0.65;
    const viewPitch = hoverPitch + scrollProgress * 0.04;
    if (
      Math.abs(viewYaw - root.rotation.y) > 0.0005 ||
      Math.abs(viewPitch - root.rotation.x) > 0.0005
    ) {
      const smoothing = reduceMotion.matches ? 1 : 0.13;
      root.rotation.y += (viewYaw - root.rotation.y) * smoothing;
      root.rotation.x += (viewPitch - root.rotation.x) * smoothing;
      moving = true;
    }
    renderer.render(scene, camera);
    mount.dataset.motion = moving ? "moving" : "still";
    mount.dataset.viewYaw = root.rotation.y.toFixed(5);
    if (moving) requestRender();
  }
  function caption() {
    if (status)
      status.textContent = exploded
        ? "A good idea, taken apart."
        : `${pieces.length} pieces. A small beginning.`;
    mount.dataset.pieceCount = String(pieces.length);
    mount.dataset.exploded = String(exploded);
  }
  function transition(piece: Piece, index: number, intro = false) {
    piece.to.copy(piece.rest);
    piece.toRotation.copy(piece.restRotation);
    if (exploded) {
      const direction = Math.atan2(piece.rest.z || 0.1, piece.rest.x || 0.1);
      piece.to.x += Math.cos(direction) * (2.5 + (index % 3));
      piece.to.z += Math.sin(direction) * 2.5;
      piece.to.y += 1.5 + (index % 4) * 1.2;
      piece.toRotation.multiply(
        new Quaternion().setFromAxisAngle(
          new Vector3(0, 0, 1),
          (index % 2 ? 1 : -1) * 0.16,
        ),
      );
    }
    if (intro && !reduceMotion.matches) {
      piece.group.position.y += 12 + (index % 5);
      piece.group.rotation.z += index % 2 ? 0.22 : -0.22;
    }
    piece.from.copy(piece.group.position);
    piece.fromRotation.copy(piece.group.quaternion);
    piece.start = performance.now() + (reduceMotion.matches ? 0 : index * 13);
    piece.duration = reduceMotion.matches ? 0 : intro ? 1050 : 700;
    if (!piece.duration) {
      piece.group.position.copy(piece.to);
      piece.group.quaternion.copy(piece.toRotation);
    }
  }
  function resize() {
    const width = mount.clientWidth;
    const height = mount.clientHeight;
    if (!width || !height) return;
    renderer.setSize(width, height);
    const aspect = width / height;
    const span = Math.max(19.5, 24 / aspect);
    camera.left = (-span * aspect) / 2;
    camera.right = (span * aspect) / 2;
    camera.top = span / 2;
    camera.bottom = -span / 2;
    camera.updateProjectionMatrix();
    updateScroll();
  }
  function updateScroll() {
    const hero = mount.closest<HTMLElement>(".hero");
    scrollProgress = reduceMotion.matches
      ? 0
      : Math.min(
          1,
          Math.max(
            0,
            window.scrollY / (hero?.offsetHeight || window.innerHeight),
          ),
        );
    mount.dataset.scrollProgress = scrollProgress.toFixed(3);
    requestRender();
  }
  window.addEventListener("scroll", updateScroll, {
    passive: true,
    signal: eventAbort.signal,
  });
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(mount);
  const intersection = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    if (!visible && frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    if (visible) requestRender();
  });
  intersection.observe(mount);
  document.addEventListener("visibilitychange", requestRender, {
    signal: eventAbort.signal,
  });
  reduceMotion.addEventListener(
    "change",
    () => {
      if (reduceMotion.matches) {
        hoverYaw = 0;
        hoverPitch = 0;
        for (const piece of pieces) transition(piece, 0);
      }
      updateScroll();
    },
    { signal: eventAbort.signal },
  );
  let drag: { x: number; yaw: number; id: number } | undefined;
  canvas.addEventListener(
    "pointerdown",
    (event) => {
      if (event.button !== 0) return;
      hoverYaw = 0;
      hoverPitch = 0;
      drag = { x: event.clientX, yaw: targetYaw, id: event.pointerId };
      canvas.setPointerCapture(event.pointerId);
    },
    { signal: eventAbort.signal },
  );
  canvas.addEventListener(
    "pointermove",
    (event) => {
      if (drag && drag.id === event.pointerId) {
        targetYaw = drag.yaw + (event.clientX - drag.x) * 0.009;
      } else if (event.pointerType === "mouse" && !reduceMotion.matches) {
        const rect = canvas.getBoundingClientRect();
        hoverYaw = ((event.clientX - rect.left) / rect.width - 0.5) * 0.32;
        hoverPitch = ((event.clientY - rect.top) / rect.height - 0.5) * 0.07;
      } else return;
      requestRender();
    },
    { signal: eventAbort.signal },
  );
  canvas.addEventListener(
    "pointerleave",
    () => {
      hoverYaw = 0;
      hoverPitch = 0;
      requestRender();
    },
    { signal: eventAbort.signal },
  );
  canvas.addEventListener(
    "pointerup",
    () => {
      drag = undefined;
    },
    { signal: eventAbort.signal },
  );
  canvas.addEventListener(
    "pointercancel",
    () => {
      drag = undefined;
    },
    { signal: eventAbort.signal },
  );
  for (const button of buttons) {
    button.disabled = false;
    button.addEventListener(
      "click",
      () => {
        if (button.dataset.sceneAction === "rotate") targetYaw += Math.PI / 4;
        else if (button.dataset.sceneAction === "explode") {
          exploded = !exploded;
          pieces.slice(1).forEach((piece, index) => transition(piece, index));
          button.textContent = exploded ? "Build it" : "Take apart";
          const add = buttons.find(
            (candidate) => candidate.dataset.sceneAction === "add",
          );
          if (add) add.disabled = exploded || added >= 5;
        } else if (!exploded && added < 5) {
          const colors: ColorName[] = [
            "yellow",
            "red",
            "mint",
            "blue",
            "yellow",
          ];
          const piece = brick(
            2,
            2,
            colors[added],
            -5,
            1.68 + added * 1.06,
            3.8,
          );
          transition(piece, 0, true);
          added++;
          if (added === 5) {
            button.disabled = true;
            button.textContent = "Nice little tower!";
          }
        }
        caption();
        requestRender();
      },
      { signal: eventAbort.signal },
    );
  }
  canvas.addEventListener(
    "webglcontextlost",
    (event) => {
      event.preventDefault();
      mount.classList.add("scene-unavailable");
      for (const button of buttons) button.disabled = true;
      if (status)
        status.textContent = "The guide is ready, even without WebGL.";
    },
    { signal: eventAbort.signal },
  );
  pieces.slice(1).forEach((piece, index) => transition(piece, index, true));
  caption();
  resize();
  return () => {
    disposed = true;
    cancelAnimationFrame(frame);
    eventAbort.abort();
    resizeObserver.disconnect();
    intersection.disconnect();
    for (const geometry of geometries.values()) geometry.dispose();
    for (const value of materials.values()) value.dispose();
    for (const value of extraMaterials) value.dispose();
    light.shadow.dispose();
    renderer.dispose();
    canvas.remove();
  };
}
