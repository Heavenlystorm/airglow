"""Sensor picking on hardware other than the captured PC, using small trees in LibreHardwareMonitor's data.json
shape and its sensor names. Real captures from more PCs are welcome: see CONTRIBUTING.md."""

import unittest

from airglow import sensors


def sensor(name, value, sid):
    return {"Text": name, "Value": value, "SensorId": sid, "Children": []}


def group(name, *children):
    return {"Text": name, "Value": "", "Children": list(children)}


def hardware(name, hid, img, *groups):
    return {"Text": name, "Value": "", "HardwareId": hid, "ImageURL": f"images_icon/{img}", "Children": list(groups)}


def tree(*hw):
    return {"Text": "Sensor", "Children": [{"Text": "TEST-PC", "Value": "", "Children": list(hw)}]}


INTEL_AMD = tree(
    hardware("Intel Core i7-14700K", "/intelcpu/0", "cpu.png",
             group("Temperatures", sensor("CPU Core #1", "61,0 °C", "/intelcpu/0/temperature/0"),
                   sensor("CPU Package", "66,0 °C", "/intelcpu/0/temperature/20"),
                   sensor("Core Max", "71,0 °C", "/intelcpu/0/temperature/21")),
             group("Load", sensor("CPU Total", "35,2 %", "/intelcpu/0/load/0")),
             group("Powers", sensor("CPU Package", "128,4 W", "/intelcpu/0/power/0"),
                   sensor("CPU Cores", "110,0 W", "/intelcpu/0/power/1"))),
    hardware("Intel(R) UHD Graphics 770", "/gpu-intel-integrated/0", "intel.png",
             group("Load", sensor("D3D 3D", "3,0 %", "/gpu-intel-integrated/0/load/0"))),
    hardware("AMD Radeon RX 7900 XT", "/gpu-amd/0", "amd.png",
             group("Temperatures", sensor("GPU Core", "58,0 °C", "/gpu-amd/0/temperature/0"),
                   sensor("GPU Hot Spot", "74,0 °C", "/gpu-amd/0/temperature/2")),
             group("Load", sensor("GPU Core", "97,0 %", "/gpu-amd/0/load/0")),
             group("Powers", sensor("GPU Package", "290,0 W", "/gpu-amd/0/power/3")),
             group("Fans", sensor("GPU Fan", "1450 RPM", "/gpu-amd/0/fan/0"))),
)

LAPTOP = tree(
    hardware("Intel Core Ultra 7 155H", "/intelcpu/0", "cpu.png",
             group("Temperatures", sensor("CPU Package", "52,0 °C", "/intelcpu/0/temperature/0")),
             group("Powers", sensor("CPU Package", "18,2 W", "/intelcpu/0/power/0"))),
    hardware("Intel(R) Arc(TM) Graphics", "/gpu-intel-integrated/0", "intel.png",
             group("Temperatures", sensor("GPU Core", "49,0 °C", "/gpu-intel-integrated/0/temperature/0"))),
)


class IntelCpuAmdGpuTests(unittest.TestCase):
    def setUp(self):
        self.s = sensors.parse(INTEL_AMD)

    def test_intel_package_temperature_and_power(self):
        self.assertEqual(self.s["cpu_name"], "Intel Core i7-14700K")
        self.assertEqual(self.s["cpu_temp"], 66.0)
        self.assertEqual(self.s["cpu_power"], 128.4)

    def test_discrete_amd_gpu_beats_the_integrated_one(self):
        self.assertEqual(self.s["gpu_name"], "AMD Radeon RX 7900 XT")
        self.assertEqual(self.s["gpu_temp"], 58.0)
        self.assertEqual(self.s["gpu_hotspot"], 74.0)
        self.assertEqual(self.s["gpu_power"], 290.0)
        self.assertEqual([f["name"] for f in self.s["fans"]], ["GPU fan"])

    def test_quick_picks_resolve(self):
        flat = sensors.flatten(INTEL_AMD)
        r = sensors.build(flat, {})
        self.assertEqual(r["auto/cpu_load"]["value"], 35.2)
        self.assertEqual(r["auto/gpu_load"]["value"], 97.0)
        self.assertEqual(r["auto/total_power"]["value"], 418.4)


class LaptopTests(unittest.TestCase):
    def test_no_discrete_gpu_means_no_gpu_temperature_for_fans(self):
        s = sensors.parse(LAPTOP)
        self.assertEqual(s["cpu_temp"], 52.0)
        self.assertIsNone(s["gpu_temp"])            # integrated graphics never drive the fan curve
        self.assertIsNone(s["total_power"])


if __name__ == "__main__":
    unittest.main()
