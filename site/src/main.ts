import "@fontsource-variable/outfit";
import "@fontsource-variable/manrope";
import "./style.css";

document.documentElement.classList.add("js-enabled");

const menu = document.querySelector<HTMLButtonElement>(".menu-toggle");
const nav = document.querySelector<HTMLElement>("#site-nav");
function closeMenu() {
  menu?.setAttribute("aria-expanded", "false");
  nav?.classList.remove("is-open");
}
menu?.addEventListener("click", () => {
  const expanded = menu.getAttribute("aria-expanded") !== "true";
  menu.setAttribute("aria-expanded", String(expanded));
  nav?.classList.toggle("is-open", expanded);
});
nav?.addEventListener("click", (event) => {
  if ((event.target as Element).closest("a")) closeMenu();
});
document.addEventListener("keydown", (event) => {
  if (
    event.key === "Escape" &&
    menu?.getAttribute("aria-expanded") === "true"
  ) {
    closeMenu();
    menu.focus();
  }
});

for (const button of document.querySelectorAll<HTMLButtonElement>(
  "[data-copy]",
)) {
  button.addEventListener("click", async () => {
    const target = document.getElementById(button.dataset.copy!);
    if (!target) return;
    const status = document.querySelector<HTMLElement>("#copy-status");
    const original = button.textContent;
    try {
      await navigator.clipboard.writeText(target.textContent ?? "");
      button.textContent = "Copied";
      if (status) status.textContent = "Copied to clipboard.";
      window.setTimeout(() => {
        button.textContent = original;
      }, 1800);
    } catch {
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(target);
      selection?.removeAllRanges();
      selection?.addRange(range);
      button.textContent = "Text selected";
      if (status)
        status.textContent =
          "Clipboard access is unavailable. Text selected; use your copy shortcut.";
    }
  });
}

const tabs = [...document.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
function selectTab(tab: HTMLButtonElement, focus = false) {
  for (const candidate of tabs) {
    const selected = candidate === tab;
    candidate.setAttribute("aria-selected", String(selected));
    candidate.tabIndex = selected ? 0 : -1;
    const panel = document.getElementById(
      candidate.getAttribute("aria-controls")!,
    );
    if (panel) panel.hidden = !selected;
  }
  if (focus) tab.focus();
}
tabs.forEach((tab, index) => {
  tab.addEventListener("click", () => selectTab(tab));
  tab.addEventListener("keydown", (event) => {
    let next: number;
    if (event.key === "ArrowRight") next = (index + 1) % tabs.length;
    else if (event.key === "ArrowLeft")
      next = (index + tabs.length - 1) % tabs.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = tabs.length - 1;
    else return;
    event.preventDefault();
    selectTab(tabs[next], true);
  });
});

const mount = document.querySelector<HTMLElement>("[data-brick-scene]");
if (mount) {
  import("./viewer-scene")
    .then(async ({ mountBrickScene }) => {
      const dispose = await mountBrickScene(mount);
      window.addEventListener("pagehide", (event) => {
        if (!event.persisted) dispose();
      });
    })
    .catch(() => {
      mount.classList.add("scene-unavailable");
      const status = document.querySelector("[data-scene-status]");
      if (status)
        status.textContent = "The guide is ready, even without WebGL.";
    });
}
