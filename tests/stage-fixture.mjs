import { readFileSync } from "node:fs";
import vm from "node:vm";

// The layout helpers and interaction panel ship alongside the painter. Only its host
// UI and telemetry adapters come from the test's synthetic browser.
export function linkStage(stage, context, core, readings) {
  const modules = new Map([["./core.js", core], ["./readings.js", readings]]);
  for (const file of ["arrange-model.js", "stage-arrange.js"]) {
    const source = readFileSync(new URL("../web/js/" + file, import.meta.url), "utf8");
    modules.set("./" + file, new vm.SourceTextModule(source, { context, identifier: file }));
  }
  return stage.link(specifier => {
    if (!modules.has(specifier)) throw new Error("Unexpected stage dependency: " + specifier);
    return modules.get(specifier);
  });
}
