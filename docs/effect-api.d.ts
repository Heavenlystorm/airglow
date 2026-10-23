// Airglow effect API, version 1 — type declarations for editors.
//
// Not shipped with the app: copy this next to an effect's script (or reference it) and an editor will complete
// `airglow`, `ctx`, `frame` and `led`. The format itself is in docs/EFFECT-STUDIO.md.

type AirglowHex = `#${string}`;

interface AirglowAudio {
  /** 0..1, the whole analysed signal. */
  level: number;
  /** 0..1, the low end. */
  bass: number;
  /** True on the frame a beat starts. */
  beat: boolean;
  /** 64 log-spaced bands, each 0..1. */
  bands: Float32Array;
}

interface AirglowFrame {
  /** Seconds since the effect started (or the clock was restarted in the Studio). */
  time: number;
  /** Seconds since the previous frame. */
  dt: number;
  /** null without the audio permission, or while nothing is playing. */
  audio: AirglowAudio | null;
}

interface AirglowLed {
  /** 0..1 across the case (0..1.6 in canvas units would be px; x is the normalised value). */
  x: number;
  y: number;
  /** The same position in the 320 x 200 effect canvas. */
  px: number;
  py: number;
  /** 0..1 around this LED's own fan or along its own strip. */
  local: number;
  /** Which fan in its zone (0 when it is a strip or grid). */
  fan: number;
  /** The zone's name, e.g. "Front fans". */
  zone: string | null;
  /** The area it sits in: front, top, rear, bottom, side, cooler, ram, board, gpu, desk, or null. */
  area: string | null;
  /** Its index in the whole rig. */
  index: number;
}

interface AirglowZone {
  name: string;
  area: string | null;
  count: number;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

interface AirglowRig {
  count: number;
  zones: AirglowZone[];
}

type AirglowValue = number | string | boolean | AirglowHex | AirglowHex[];

interface AirglowCtx {
  /** 320 x 200, same 1.6 : 1 shape as the stage. 2d or webgl2, whichever the effect asks for first. */
  canvas: OffscreenCanvas;
  width: number;
  height: number;
  /** The effect's controls, by id. */
  params: Record<string, AirglowValue>;
  /** Where the LEDs are, so an effect can adapt to the rig it is on. */
  rig: AirglowRig;
  /** Same as console.log: shows under the Studio's preview. */
  log(...args: unknown[]): void;
}

interface AirglowEffect {
  init?(ctx: AirglowCtx): void;
  /** Draw on ctx.canvas; each LED takes the colour under it. */
  render?(ctx: AirglowCtx, frame: AirglowFrame): void;
  /** Or answer per LED, 0..255 each. */
  point?(led: AirglowLed, frame: AirglowFrame): number[];
  /** Once per frame, before point() is called for the LEDs. */
  frame?(frame: AirglowFrame): void;
  onParamChange?(id: string, value: AirglowValue, ctx: AirglowCtx): void;
  dispose?(ctx: AirglowCtx): void;
}

interface AirglowNumberParam {
  label?: string;
  type: "number";
  min: number;
  max: number;
  step?: number;
  default: number;
}

interface AirglowColorParam {
  label?: string;
  type: "color";
  default: AirglowHex;
}

interface AirglowColorsParam {
  label?: string;
  type: "colors";
  /** 2 to 4 colours. */
  default: AirglowHex[];
}

interface AirglowBooleanParam {
  label?: string;
  type: "boolean";
  default: boolean;
}

interface AirglowSelectParam {
  label?: string;
  type: "select";
  options: string[];
  default: string;
}

type AirglowParam =
  | AirglowNumberParam
  | AirglowColorParam
  | AirglowColorsParam
  | AirglowBooleanParam
  | AirglowSelectParam;

interface AirglowLayer {
  /** Stable within the effect, so a control keeps its setting when the stack is reordered. */
  id?: string;
  type: string;
  enabled?: boolean;
  /** 0..100. */
  opacity?: number;
  blend?: "normal" | "add" | "screen" | "multiply";
  /** "all" or an area (front, top, rear, bottom, side, cooler, ram, board, gpu, desk). */
  mask?: string;
  /** Take this layer's colour (or colours) from the effect-wide palette. */
  palette?: boolean;
  /** Follow the music. */
  react?: { what: "strength" | "speed"; amount: number };
  params?: Record<string, AirglowValue>;
  /** setting -> label, for the layer's own controls. */
  expose?: Record<string, string>;
}

interface AirglowMaster {
  /** 2 to 4 colours. */
  palette?: AirglowHex[];
  /** 0..200, 100 is normal. */
  speed?: number;
  /** 0..100. */
  brightness?: number;
  /** 0..100, how strongly the music moves the layers. */
  react?: number;
}

interface AirglowLayerConfig {
  v?: 2;
  layers: AirglowLayer[];
  master?: AirglowMaster;
  /** knob -> label, for effect-wide controls (palette, speed, brightness, react). */
  expose?: Record<string, string>;
}

declare const airglow: {
  effect(def: AirglowEffect): void;
  /** Declare controls next to the code; Airglow writes them into the file when it saves. */
  params(specs: Record<string, AirglowParam>): void;
};

/** The layer renderer that comes with every effect. */
declare const AirglowLayers: {
  effect(config: AirglowLayerConfig | AirglowLayer[], defaults?: Record<string, Record<string, AirglowValue>>): AirglowEffect;
  TYPES: Record<string, (params: Record<string, AirglowValue>) => unknown>;
  hsv(h: number, s: number, v: number): number[];
  rgb(hex: AirglowHex): number[];
  hz(speed: number, lo: number, hi: number): number;
  MASTER: AirglowMaster & { palette: AirglowHex[] };
};
