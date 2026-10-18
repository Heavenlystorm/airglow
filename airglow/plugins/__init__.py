"""Device plugins: one module per controller family, found and loaded automatically.

Adding a controller family means adding one file here. Nothing else in Airglow names it: detection, the OpenRGB
hand-off, the Devices tab and per-device settings all come from what the module declares.

A plugin module defines:

    NAME          "NZXT HUE 2 family", shown in diagnostics and the Devices tab
    VENDOR        "NZXT"
    TRANSPORT     "hid" (the only transport so far)
    ID_PREFIX     stable device id for the first controller found ("nzxt-hub"); later ones get "-2", "-3". Saved
                  zone settings are keyed by it, so never change it once released.
    PRODUCTS      list of Product: every USB id the plugin drives, with per-model facts
    create(info, product, dev_id, backend) -> hw.Device
                  build the device for one matched controller. info is the hid.enumerate() entry.

and optionally:

    validate(info, product) -> bool
                  accept or reject one USB interface of a matching controller (default: the product's interface,
                  or the first interface seen when it names none)
    OPENRGB_SKIP  name fragments of OpenRGB devices this plugin already drives, so two programs never fight over
                  the same LEDs. Only applied when one of the plugin's controllers is actually present and lights
                  something (Device.lighting).
    OPENRGB_SKIP_TYPES
                  OpenRGB device types (openrgb.DEVICE_TYPES, e.g. "Motherboard") OPENRGB_SKIP is limited to, so a
                  brand-wide fragment never hides the same brand's other devices. Default: any type.
    Product.openrgb_detector
                  the name of OpenRGB's detector for a product. When Airglow's helper setup installs OpenRGB, it
                  switches off the detectors of controllers Airglow lights on this PC, so OpenRGB never opens them.
    PARAMETERS    per-device settings, in the same schema as effect parameters (types range, choice, toggle,
                  color). The Devices tab draws them; saved values reach the device through Device.set_params().

Plugins must not do I/O at import time: they are imported on every start, with or without their hardware.
"""

import importlib
import logging
import pkgutil
from dataclasses import dataclass, field

log = logging.getLogger(__name__)

REQUIRED = ("NAME", "VENDOR", "TRANSPORT", "ID_PREFIX", "PRODUCTS", "create")
TRANSPORTS = ("hid",)


@dataclass(frozen=True)
class Product:
    vid: int
    pid: int
    name: str
    interface: int | None = None        # the HID interface to use when the controller exposes several
    experimental: bool = False          # same protocol as a tested model, but not confirmed with Airglow yet
    note: str | None = None
    openrgb_detector: str | None = None # OpenRGB's detector for this controller, switched off when Airglow sets
                                        # OpenRGB up and lights it itself (several products can share one)
    facts: dict = field(default_factory=dict)    # anything model-specific the plugin needs (channel counts, ...)


class PluginError(ValueError):
    pass


def check(mod):
    """Raise PluginError if a module doesn't meet the contract above."""
    name = mod.__name__.rpartition(".")[2]
    missing = [a for a in REQUIRED if not hasattr(mod, a)]
    if missing:
        raise PluginError(f"plugin {name} is missing {', '.join(missing)}")
    if mod.TRANSPORT not in TRANSPORTS:
        raise PluginError(f"plugin {name} uses unknown transport {mod.TRANSPORT!r}")
    if not mod.PRODUCTS or not all(isinstance(p, Product) for p in mod.PRODUCTS):
        raise PluginError(f"plugin {name} needs a non-empty PRODUCTS list of Product")
    ids = [(p.vid, p.pid, p.interface) for p in mod.PRODUCTS]
    if len(ids) != len(set(ids)):
        raise PluginError(f"plugin {name} lists a product twice")
    for spec in getattr(mod, "PARAMETERS", []):
        if not {"id", "label", "type", "default"} <= set(spec):
            raise PluginError(f"plugin {name}: parameter {spec.get('id')!r} needs id, label, type and default")


def load():
    """Every plugin module in this package, sorted by name. A broken plugin is logged and left out; it must never
    stop the others (or OpenRGB devices) from working."""
    found = []
    for info in sorted(pkgutil.iter_modules(__path__), key=lambda m: m.name):
        if info.name.startswith("_"):
            continue
        try:
            mod = importlib.import_module(f"{__name__}.{info.name}")
            check(mod)
        except Exception as e:
            log.error("device plugin %s not loaded: %s", info.name, e)
            continue
        found.append(mod)
    prefixes = [m.ID_PREFIX for m in found]
    dupes = {p for p in prefixes if prefixes.count(p) > 1}
    if dupes:
        log.error("device plugins share an ID_PREFIX (%s); saved zone settings would mix", ", ".join(sorted(dupes)))
    return found


def default_validate(info, product):
    return product.interface is None or info.get("interface_number") == product.interface


def match(plugins, hid_list):
    """(plugin, product, hid entry) for every physical controller in an hid.enumerate() list. One entry per
    controller: the interface validate() accepts, and only the first of several when the product names none."""
    out, seen = [], set()
    for plugin in plugins:
        validate = getattr(plugin, "validate", default_validate)
        for product in plugin.PRODUCTS:
            for info in hid_list:
                if (info.get("vendor_id"), info.get("product_id")) != (product.vid, product.pid):
                    continue
                if not validate(info, product):
                    continue
                key = (product.vid, product.pid, info.get("serial_number") or "")
                if key in seen and product.interface is None:
                    continue                       # another interface of a controller we already have
                seen.add(key)
                out.append((plugin, product, info))
    return out


def catalog(plugins=None):
    """What the plugins support, for diagnostics and docs."""
    return [{"plugin": m.__name__.rpartition(".")[2], "name": m.NAME, "vendor": m.VENDOR, "transport": m.TRANSPORT,
             "products": [{"vid": f"{p.vid:04X}", "pid": f"{p.pid:04X}", "name": p.name,
                           "experimental": p.experimental} for p in m.PRODUCTS]}
            for m in (plugins if plugins is not None else load())]
