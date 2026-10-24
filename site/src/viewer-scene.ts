// Input/animation adapter only: catalog geometry, WebGL rendering, lighting,
// shadows and the actual scene live in ergo-client, exactly as in the navigator.
interface Preview {
  render(yaw: number, pitch: number, explosion: number, assembly: number): void;
  piece_count(): number;
  add_brick(): number;
  free(): void;
}
interface PreviewModule {
  default(): Promise<unknown>;
  LandingPreview: new (canvas: HTMLCanvasElement) => Preview;
}

export async function mountBrickScene(mount: HTMLElement): Promise<() => void> {
  const url = `${import.meta.env.BASE_URL}engine/v${import.meta.env.VITE_ENGINE_VERSION}/ergo_client.js`;
  const wasm = (await import(/* @vite-ignore */ url)) as PreviewModule;
  await wasm.default();
  const canvas = document.createElement("canvas");
  canvas.setAttribute("role", "img");
  canvas.setAttribute(
    "aria-label",
    "Interactive catalog bricks rendered by Ergo's navigator engine. Move the mouse, drag or scroll to change the view; use the buttons to add bricks or take the model apart.",
  );
  canvas.style.touchAction = "pan-y";
  mount.append(canvas);
  let engine: Preview;
  try {
    engine = new wasm.LandingPreview(canvas);
  } catch (error) {
    canvas.remove();
    throw error;
  }
  mount.classList.add("scene-ready");

  const status = document.querySelector<HTMLElement>("[data-scene-status]");
  const buttons = [
    ...document.querySelectorAll<HTMLButtonElement>("[data-scene-action]"),
  ];
  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const abort = new AbortController();
  let disposed = false;
  let visible = true;
  let frame = 0;
  let targetYaw = 0.73;
  let yaw = targetYaw;
  let pitch = 0.65;
  let hoverYaw = 0;
  let hoverPitch = 0;
  let scrollProgress = 0;
  let explosion = 0;
  let targetExplosion = 0;
  let added = 0;
  const start = performance.now();

  function requestRender() {
    if (!frame && !disposed && visible && !document.hidden)
      frame = requestAnimationFrame(render);
  }
  function caption() {
    const count = engine.piece_count();
    const flashlight = canvas.dataset.lighting === "flashlight";
    const label = `${count} catalog bricks. ${flashlight ? "Fixed night light" : "Real-time sun"}.`;
    if (status && status.textContent !== label) status.textContent = label;
    mount.dataset.pieceCount = String(count);
    mount.dataset.exploded = String(targetExplosion > 0);
    mount.dataset.renderer = "ergo-webgl2";
  }
  function render(now: number) {
    frame = 0;
    if (disposed || !visible || document.hidden) return;
    const assembly = reduceMotion.matches
      ? 1
      : Math.min(1, (now - start) / 1500);
    const goalYaw = targetYaw + hoverYaw + scrollProgress * 0.65;
    const goalPitch = 0.65 + hoverPitch + scrollProgress * 0.04;
    const smoothing = reduceMotion.matches ? 1 : 0.13;
    yaw += (goalYaw - yaw) * smoothing;
    pitch += (goalPitch - pitch) * smoothing;
    explosion += (targetExplosion - explosion) * smoothing;
    if (Math.abs(goalYaw - yaw) < 0.0005) yaw = goalYaw;
    if (Math.abs(goalPitch - pitch) < 0.0005) pitch = goalPitch;
    if (Math.abs(targetExplosion - explosion) < 0.0005)
      explosion = targetExplosion;
    engine.render(yaw, pitch, explosion, assembly);
    const moving =
      assembly < 1 ||
      yaw !== goalYaw ||
      pitch !== goalPitch ||
      explosion !== targetExplosion;
    mount.dataset.motion = moving ? "moving" : "still";
    mount.dataset.viewYaw = yaw.toFixed(5);
    caption();
    if (moving) requestRender();
  }
  function scroll() {
    const hero = mount.closest<HTMLElement>(".hero");
    scrollProgress = reduceMotion.matches
      ? 0
      : Math.min(
          1,
          Math.max(0, window.scrollY / (hero?.offsetHeight || innerHeight)),
        );
    mount.dataset.scrollProgress = scrollProgress.toFixed(3);
    requestRender();
  }
  window.addEventListener("scroll", scroll, {
    passive: true,
    signal: abort.signal,
  });
  document.addEventListener("visibilitychange", requestRender, {
    signal: abort.signal,
  });
  reduceMotion.addEventListener(
    "change",
    () => {
      hoverYaw = hoverPitch = 0;
      scroll();
    },
    { signal: abort.signal },
  );
  const resize = new ResizeObserver(requestRender);
  resize.observe(mount);
  const intersection = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    if (!visible && frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    if (visible) requestRender();
  });
  intersection.observe(mount);
  // The sun tracks UTC even when the visitor leaves the illustration untouched.
  const minute = window.setInterval(requestRender, 60000);
  let drag: { x: number; yaw: number; id: number } | undefined;
  canvas.addEventListener(
    "pointerdown",
    (event) => {
      if (event.button !== 0) return;
      hoverYaw = hoverPitch = 0;
      drag = { x: event.clientX, yaw: targetYaw, id: event.pointerId };
      canvas.setPointerCapture(event.pointerId);
    },
    { signal: abort.signal },
  );
  canvas.addEventListener(
    "pointermove",
    (event) => {
      if (drag && drag.id === event.pointerId)
        targetYaw = drag.yaw + (event.clientX - drag.x) * 0.009;
      else if (event.pointerType === "mouse" && !reduceMotion.matches) {
        const rect = canvas.getBoundingClientRect();
        hoverYaw = ((event.clientX - rect.left) / rect.width - 0.5) * 0.32;
        hoverPitch = ((event.clientY - rect.top) / rect.height - 0.5) * 0.07;
      } else return;
      requestRender();
    },
    { signal: abort.signal },
  );
  canvas.addEventListener(
    "pointerleave",
    () => {
      hoverYaw = hoverPitch = 0;
      requestRender();
    },
    { signal: abort.signal },
  );
  canvas.addEventListener(
    "pointerup",
    () => {
      drag = undefined;
    },
    { signal: abort.signal },
  );
  canvas.addEventListener(
    "pointercancel",
    () => {
      drag = undefined;
    },
    { signal: abort.signal },
  );
  for (const button of buttons) {
    button.disabled = false;
    button.addEventListener(
      "click",
      () => {
        if (button.dataset.sceneAction === "rotate") targetYaw += Math.PI / 4;
        else if (button.dataset.sceneAction === "explode") {
          targetExplosion = targetExplosion ? 0 : 1;
          button.textContent = targetExplosion ? "Build it" : "Take apart";
          const add = buttons.find(
            (item) => item.dataset.sceneAction === "add",
          );
          if (add) add.disabled = targetExplosion > 0 || added >= 5;
        } else if (!targetExplosion && added < 5) {
          engine.add_brick();
          added++;
          if (added === 5) {
            button.disabled = true;
            button.textContent = "Nice little tower!";
          }
        }
        caption();
        requestRender();
      },
      { signal: abort.signal },
    );
  }
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    cancelAnimationFrame(frame);
    clearInterval(minute);
    abort.abort();
    resize.disconnect();
    intersection.disconnect();
    engine.free();
    canvas.remove();
  };
  canvas.addEventListener(
    "webglcontextlost",
    (event) => {
      event.preventDefault();
      mount.classList.add("scene-unavailable");
      for (const button of buttons) button.disabled = true;
      if (status)
        status.textContent = "The guide is ready, even without WebGL.";
      dispose();
    },
    { signal: abort.signal },
  );
  scroll();
  return dispose;
}
