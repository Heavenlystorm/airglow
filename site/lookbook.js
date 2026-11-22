// Each 32-pixel row contains one frame from Airglow’s own renderer at the recipe brightness.
for (const control of document.querySelectorAll(".scrub")) {
  const slider = control.querySelector("input");
  const output = control.querySelector("output");
  const strip = control.closest(".emitter").querySelector(".led-row");
  const showFrame = () => {
    const frame = slider.valueAsNumber;
    const seconds = (frame / 10).toFixed(1);
    strip.style.backgroundPositionY = `${frame / Number(slider.max) * 100}%`;
    slider.setAttribute("aria-valuetext", `${seconds} seconds`);
    output.value = `${seconds} s`;
  };
  slider.addEventListener("input", showFrame);
  showFrame();
  control.hidden = false;
}

// On desktop an expanded chapter supplies the screenshot beside the list.
const wide = window.matchMedia("(min-width: 721px)");
for (const chapter of document.querySelectorAll(".chapter")) {
  chapter.querySelector("summary").addEventListener("click", event => {
    if (wide.matches && chapter.open) event.preventDefault();
  });
}
wide.addEventListener("change", () => {
  if (wide.matches && !document.querySelector(".chapter[open]")) {
    document.querySelector(".chapter").open = true;
  }
});
