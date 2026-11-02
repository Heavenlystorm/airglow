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


class AliasAccuracyTests(unittest.TestCase):
    def readings(self, *devices, system=None):
        return sensors.build(sensors.flatten(tree(*devices)), system or {})

    def test_drive_source_follows_the_selected_composite_temperature(self):
        r = self.readings(
            hardware("First SSD", "/nvme/0", "hdd.png",
                     group("Temperatures", sensor("Temperature", "31 C", "/nvme/0/temperature/0"))),
            hardware("Second SSD", "/nvme/1", "hdd.png",
                     group("Temperatures", sensor("Composite", "53 C", "/nvme/1/temperature/0"))),
        )
        self.assertEqual(r["auto/ssd_temp"]["value"], 53)
        self.assertEqual(r["auto/ssd_temp"]["source"], "Second SSD")
        self.assertEqual(r["auto/ssd_temp"].get("source_id"), "/nvme/1/temperature/0")

    def test_windows_cpu_load_fallback_is_attributed_to_windows(self):
        r = self.readings(
            hardware("Processor", "/intelcpu/0", "cpu.png",
                     group("Load", sensor("CPU Core #1", "75 %", "/intelcpu/0/load/1"))),
            system={"cpu_load": 0.0},
        )
        self.assertEqual(r["auto/cpu_load"]["value"], 0)
        self.assertEqual(r["auto/cpu_load"]["source"], "Windows")
        self.assertEqual(r["auto/cpu_load"].get("source_id"), "sys/cpu_load")

    def test_hotspot_does_not_substitute_core_or_memory_temperatures(self):
        for name in ("GPU Core", "GPU Memory Junction", "GPU Memory Hot Spot"):
            with self.subTest(name=name):
                flat = sensors.flatten(tree(hardware("Graphics", "/gpu-nvidia/0", "nvidia.png",
                    group("Temperatures", sensor(name, "60 C", "/gpu-nvidia/0/temperature/0")))))
                self.assertIsNone(sensors.summary(flat)["gpu_hotspot"])
                reading = sensors.build(flat, {})["auto/gpu_hotspot"]
                self.assertIsNone(reading["value"])
                self.assertIsNone(reading["source"])
                self.assertNotIn("source_id", reading)

    def test_hotspot_source_follows_the_card_with_a_hotspot_sensor(self):
        r = self.readings(
            hardware("First GPU", "/gpu-nvidia/0", "nvidia.png",
                     group("Temperatures", sensor("GPU Core", "40 C", "/gpu-nvidia/0/temperature/0"))),
            hardware("Second GPU", "/gpu-amd/0", "amd.png",
                     group("Temperatures", sensor("GPU Hot Spot", "78 C", "/gpu-amd/0/temperature/1"))),
        )
        self.assertEqual(r["auto/gpu_hotspot"]["value"], 78)
        self.assertEqual(r["auto/gpu_hotspot"]["source"], "Second GPU")
        self.assertEqual(r["auto/gpu_hotspot"].get("source_id"), "/gpu-amd/0/temperature/1")

    def test_summary_names_follow_the_selected_temperature_device(self):
        flat = sensors.flatten(tree(
            hardware("First GPU", "/gpu-nvidia/0", "nvidia.png",
                     group("Temperatures", sensor("GPU Core", "40 C", "/gpu-nvidia/0/temperature/0"))),
            hardware("Second GPU", "/gpu-nvidia/1", "nvidia.png",
                     group("Temperatures", sensor("GPU Core", "50 C", "/gpu-nvidia/1/temperature/0"))),
        ))
        self.assertEqual(sensors.summary(flat)["gpu_temp"], 40)
        self.assertEqual(sensors.summary(flat)["gpu_name"], "First GPU")

    def test_gpu_load_does_not_substitute_memory_or_video_load(self):
        r = self.readings(hardware("Graphics", "/gpu-nvidia/0", "nvidia.png",
            group("Load", sensor("GPU Memory Controller", "90 %", "/gpu-nvidia/0/load/1"),
                  sensor("GPU Video Engine", "60 %", "/gpu-nvidia/0/load/2"))))
        self.assertIsNone(r["auto/gpu_load"]["value"])
        self.assertEqual(r["/gpu-nvidia/0/load/1"]["value"], 90)

    def test_component_power_is_not_reported_as_package_or_total_power(self):
        flat = sensors.flatten(tree(
            hardware("Processor", "/intelcpu/0", "cpu.png",
                     group("Powers", sensor("CPU Cores", "30 W", "/intelcpu/0/power/1"))),
            hardware("Graphics", "/gpu-nvidia/0", "nvidia.png",
                     group("Powers", sensor("GPU Memory", "80 W", "/gpu-nvidia/0/power/1"))),
        ))
        summary, readings = sensors.summary(flat), sensors.build(flat, {})
        for key in ("cpu_power", "gpu_power", "total_power"):
            with self.subTest(key=key):
                self.assertIsNone(summary[key])
                self.assertIsNone(readings["auto/" + key]["value"])

    def test_total_power_keeps_both_input_sensor_ids(self):
        r = sensors.build(sensors.flatten(INTEL_AMD), {})["auto/total_power"]
        self.assertEqual(r["value"], 418.4)
        self.assertEqual(r.get("source_ids"), ["/intelcpu/0/power/0", "/gpu-amd/0/power/3"])

    def test_cpu_clock_averages_only_raw_cores_on_one_processor(self):
        r = self.readings(
            hardware("First CPU", "/intelcpu/0", "cpu.png",
                     group("Clocks", sensor("Bus Speed", "100 MHz", "/intelcpu/0/clock/0"),
                           sensor("CPU Core #1", "4000 MHz", "/intelcpu/0/clock/1"),
                           sensor("CPU Core #2", "5000 MHz", "/intelcpu/0/clock/2"),
                           sensor("CPU Core #1 (Effective)", "500 MHz", "/intelcpu/0/clock/3"))),
            hardware("Second CPU", "/intelcpu/1", "cpu.png",
                     group("Clocks", sensor("CPU Core #1", "8000 MHz", "/intelcpu/1/clock/1"))),
        )
        self.assertEqual(r["auto/cpu_clock"]["value"], 4500)
        self.assertEqual(r["auto/cpu_clock"]["source"], "First CPU")
        self.assertEqual(r["auto/cpu_clock"].get("source_ids"),
                         ["/intelcpu/0/clock/1", "/intelcpu/0/clock/2"])

    def test_cpu_clock_prefers_the_reported_average(self):
        r = self.readings(hardware("Processor", "/amdcpu/0", "cpu.png",
            group("Clocks", sensor("Cores (Average)", "4300 MHz", "/amdcpu/0/clock/0"),
                  sensor("Core #1", "5000 MHz", "/amdcpu/0/clock/1"))))
        self.assertEqual(r["auto/cpu_clock"]["value"], 4300)
        self.assertEqual(r["auto/cpu_clock"].get("source_id"), "/amdcpu/0/clock/0")

    def test_cpu_clock_does_not_infer_an_average_from_bus_or_effective_clocks(self):
        r = self.readings(hardware("Processor", "/amdcpu/0", "cpu.png",
            group("Clocks", sensor("Bus Speed", "100 MHz", "/amdcpu/0/clock/0"),
                  sensor("Core #1 (Effective)", "500 MHz", "/amdcpu/0/clock/1"))))
        self.assertIsNone(r["auto/cpu_clock"]["value"])

    def test_vram_used_prefers_dedicated_memory_over_shared_system_memory(self):
        r = self.readings(hardware("Graphics", "/gpu-nvidia/0", "nvidia.png",
            group("SmallData", sensor("D3D Shared Memory Used", "1024 MB", "/gpu-nvidia/0/smalldata/0"),
                  sensor("GPU Memory Used", "2048 MB", "/gpu-nvidia/0/smalldata/1"),
                  sensor("GPU Memory Total", "8192 MB", "/gpu-nvidia/0/smalldata/2"))))
        self.assertEqual(r["auto/vram_used"]["value"], 2048)
        self.assertEqual(r["auto/vram_used"]["max"], 8192)

    def test_vram_used_does_not_substitute_shared_system_memory(self):
        r = self.readings(hardware("Graphics", "/gpu-nvidia/0", "nvidia.png",
            group("SmallData", sensor("D3D Shared Memory Used", "1024 MB", "/gpu-nvidia/0/smalldata/0"))))
        self.assertIsNone(r["auto/vram_used"]["value"])

    def test_vram_falls_back_to_dedicated_memory_despite_catalog_noise_filter(self):
        r = self.readings(
            hardware("First GPU", "/gpu-nvidia/0", "nvidia.png",
                     group("Small Data", sensor("D3D Dedicated Memory Total", "4096 MB", "/gpu-nvidia/0/smalldata/1"))),
            hardware("Second GPU", "/gpu-nvidia/1", "nvidia.png",
                     group("Small Data", sensor("D3D Dedicated Memory Used", "2048 MB", "/gpu-nvidia/1/smalldata/0"),
                           sensor("D3D Dedicated Memory Total", "8192 MB", "/gpu-nvidia/1/smalldata/1"))),
        )["auto/vram_used"]
        self.assertEqual(r["value"], 2048)
        self.assertEqual(r["max"], 8192)
        self.assertEqual(r["source"], "Second GPU")
        self.assertEqual(r.get("sensor_name"), "D3D Dedicated Memory Used")

    def test_vram_capacity_belongs_to_the_same_physical_gpu(self):
        for second_total in ([], [sensor("GPU Memory Total", "24576 MB", "/gpu-nvidia/1/smalldata/2")]):
            with self.subTest(has_capacity=bool(second_total)):
                r = self.readings(
                    hardware("Identical GPU", "/gpu-nvidia/0", "nvidia.png",
                             group("SmallData", sensor("GPU Memory Total", "8192 MB", "/gpu-nvidia/0/smalldata/2"))),
                    hardware("Identical GPU", "/gpu-nvidia/1", "nvidia.png",
                             group("SmallData", sensor("GPU Memory Used", "16000 MB", "/gpu-nvidia/1/smalldata/1"),
                                   *second_total)),
                )["auto/vram_used"]
                self.assertEqual(r["value"], 16000)
                self.assertEqual(r.get("max"), 24576 if second_total else None)

    def test_temperature_limits_are_never_live_drive_or_ram_temperatures(self):
        r = self.readings(
            hardware("Drive", "/nvme/0", "hdd.png",
                     group("Temperatures", sensor("Warning Composite Temperature", "80 C", "/nvme/0/temperature/1"))),
            hardware("DIMM", "/memory/dimm/0", "ram.png",
                     group("Temperatures", sensor("Thermal Sensor High Limit", "85 C", "/memory/dimm/0/temperature/1"))),
        )
        self.assertIsNone(r["auto/ssd_temp"]["value"])
        self.assertIsNone(r["auto/ram_temp"]["value"])

    def test_coolant_source_identifies_the_selected_probe(self):
        r = self.readings(hardware("AIO cooler", "/cooler/0", "chip.png",
            group("Temperatures", sensor("Liquid", "32 C", "/cooler/0/temperature/0"))))
        self.assertEqual(r["auto/liquid_temp"]["value"], 32)
        self.assertEqual(r["auto/liquid_temp"]["source"], "AIO cooler")
        self.assertEqual(r["auto/liquid_temp"].get("source_id"), "/cooler/0/temperature/0")

    def test_group_only_sensor_types_have_normalized_catalog_units(self):
        r = sensors.build(sensors.flatten(INTEL_AMD), {})
        self.assertEqual(r["/intelcpu/0/temperature/20"]["kind"], "temperature")
        self.assertEqual(r["/intelcpu/0/temperature/20"]["unit"], "\N{DEGREE SIGN}C")
        self.assertEqual(r["/intelcpu/0/power/0"]["unit"], "W")
        self.assertEqual(r["/gpu-amd/0/fan/0"]["unit"], "RPM")


if __name__ == "__main__":
    unittest.main()
