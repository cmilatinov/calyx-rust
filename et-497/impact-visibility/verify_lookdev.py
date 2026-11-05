"""Capture actual controller-fired effects in the approved look-dev rigs.

Run from any directory after the rebuilt editor/assembly is ready:
  python logs/et-497-v2/verify_lookdev.py --palette both --closeups

All scene changes are confined to disposable play copies. This script does
not launch/build the editor, save a scene, change projectile speed, or create
any substitute projectile/flash meshes. It leaves the final play copy paused.
"""

import argparse
from datetime import datetime
import json
import os
from pathlib import Path
import subprocess


REPO = Path(__file__).resolve().parents[2]
SCENE = "sandbox/assets/lookdev/tank_direction.cxscene"
ID = "02289c92-3412-406e-a7e5-3bbb15d7041e"
CAMERA = "a85867d2-3e68-42b2-b943-ea78c7c6ddb5"
CONTROLLER = "21b467c0-9409-4a7f-b750-418809609eb1"
NIL = "00000000-0000-0000-0000-000000000000"
PALETTES = {
    "sunny": {
        "root": "9e3ac150-2304-573e-ad1c-d118cbce9400",
        "camera": "24d6b63a-6163-5f32-907e-ccc920cde978",
        "sun": "65853898-cf37-5193-8cf0-dac7a6de4405",
        "sky": "b49347a5-b896-5233-8ce5-8f42eeb2c145",
        "post": "1c3bbc6c-e2a2-5881-ba61-618fbd01b512",
        "tank": "205418b4-61c1-5128-b5d1-0d8530da6b95",
        "turret": "e6eab6da-666c-59d7-90e8-401003defe8d",
        "barrel": "c289b4ea-d9bd-507a-b643-e61cdbe77124",
        "target": "5c033103-e835-5fae-8485-daa4f18cb296",
    },
    "sunbaked": {
        "root": "cd496f39-ef4a-5a27-9c70-e13195f862d4",
        "camera": "e8e0602a-70dc-541e-889f-cbdf2b162c8f",
        "sun": "b44068c6-3d28-5be9-91f7-68ebe57e93bf",
        "sky": "44857634-e64c-5629-8585-61807040d113",
        "post": "b070c570-71f6-5394-89d6-187c8eed75cb",
        "tank": "8e09b571-b61b-5480-bf00-212948e9a8cc",
        "turret": "96038a4b-6c58-515b-b8d5-c2fa39725be7",
        "barrel": "4622c122-3c2b-53f4-adbd-95d44491baa9",
        "target": "a9d0c01f-afdc-5dd1-b9b1-5f0964ddd421",
    },
    "moonlit": {
        "root": "7a7af86e-57a4-4481-b76d-c1ea3607bbff",
        "camera": "07f056df-a0c9-4d43-8664-dd84fd573899",
        "sun": "f7d9f6f3-61ca-49c0-95c5-b86fc407b9a7",
        "sky": "67a67fd2-8c8b-4d31-942b-4e06de81c412",
        "post": "22a00725-91df-44b0-b05a-982a1dd26bba",
        "tank": "fb502b82-4062-490e-8de1-8c39cb2f1a09",
        "turret": "caa432f0-d927-4dcf-8b35-101a8e6490fa",
        "barrel": "50a620ee-c49a-4202-afc5-35b8fc0544d0",
        "target": "1ee421d8-1208-4be9-9c11-dc634c1a15cc",
    },
}


class Harness:
    def __init__(self, args):
        self.args = args
        self.ctl_path = Path(args.ctl)
        self.env = os.environ.copy()
        nightly = (Path.home() / ".rustup/toolchains/"
                   "nightly-2026-10-22-x86_64-pc-windows-msvc/"
                   "lib/rustlib/x86_64-pc-windows-msvc/lib")
        self.env["PATH"] = os.pathsep.join([
            str(self.ctl_path.parent / "deps"), str(nightly), self.env["PATH"]
        ])
        self.out = REPO / "logs/et-497-v2" / datetime.now().strftime("run-%Y%m%d-%H%M%S-%f")
        self.out.mkdir(parents=True)
        self.discovery = json.loads((REPO / "sandbox/.calyx/remote.json").read_text())
        if self.discovery["pid"] == 35800:
            raise RuntimeError("Refusing to touch the user-owned editor PID35800")
        self.port = self.discovery["port"]
        self.allow_mutations = False
        self.scene = json.loads((REPO / SCENE).read_text())
        prefab = json.loads((REPO / "sandbox/assets/game/prefabs/tank.cxprefab").read_text())
        self.game_controller = next(c[CONTROLLER] for c in prefab["data"]["components"].values()
                                    if CONTROLLER in c)
        self.steps = 0

    def ctl(self, *args):
        result = subprocess.run(
            [str(self.ctl_path), "--project", "./sandbox", "--port", str(self.port), *map(str, args)],
            cwd=REPO, env=self.env, capture_output=True, text=True, timeout=120,
        )
        entry = {"args": list(map(str,args)), "returncode": result.returncode,
                 "stdout": result.stdout, "stderr": result.stderr}
        with (self.out / "commands.jsonl").open("a", encoding="utf-8") as log:
            log.write(json.dumps(entry) + "\n")
        if result.returncode:
            raise RuntimeError(f"{args}: {result.stderr} {result.stdout}")
        value = json.loads(result.stdout)
        if isinstance(value, dict) and value.get("status") == "ok":
            return value["data"]
        return value

    def raw(self, cmd, **fields):
        return self.ctl("raw", json.dumps({"cmd": cmd, **fields}))

    def objects(self):
        return self.ctl("objects")["objects"]

    def get(self, obj, component):
        return self.ctl("component", "get", obj, component)

    def put(self, obj, component, value):
        return self.raw("set_component", object=obj, component=component, value=value)

    def add(self, obj, component):
        self.ctl("component", "add", obj, component)
        return self.get(obj, component)

    def snapshot(self, filename, value):
        (self.out / filename).write_text(json.dumps(value, indent=2), encoding="utf-8")

    def capture(self, name):
        self.ctl("focus-game")
        self.ctl("wait", "--frames", 2)
        result = self.ctl("screenshot", "--target", "game", "--out", self.out / f"{name}.png")
        self.snapshot(f"{name}.json", {"capture": result, "simulated_steps": self.steps,
                                      "info": self.ctl("info")})
        return result

    def step(self):
        self.ctl("step", "--frames", 1)
        self.steps += 1

    def descendants(self, root):
        for child in self.scene["hierarchy"].get(root, []):
            yield child
            yield from self.descendants(child)

    def target_head(self, root):
        return next(o for o in self.descendants(root)
                    if self.scene["components"][o][ID]["name"] == "Turret Head")

    @staticmethod
    def has_component(obj, name):
        return any(c == name or c.endswith("::" + name) for c in obj["components"])

    def live_rounds(self):
        return [o for o in self.objects() if self.has_component(o, "ComponentProjectile")]

    def effect(self, name):
        return next((o for o in self.objects() if o["name"] == name
                     and self.has_component(o, "ComponentCombatEffectLifetime")), None)

    def closeup(self, palette, stage, center):
        camera = PALETTES[palette]["camera"]
        original = self.ctl("object", "get", camera)["world_transform"]["position"]
        position = [center[0], center[1] + 12.0, center[2] - 12.0 * 13.7 / 34.0]
        try:
            # Keep the agreed pitch and FOV. Only the additional closeup moves.
            self.raw("set_transform", object=camera, position=position, space="world")
            self.capture(f"{palette}-{stage}-closeup")
        finally:
            self.raw("set_transform", object=camera, position=original, space="world")

    def run_palette(self, palette):
        cfg = PALETTES[palette]
        assert self.ctl("info")["assemblies_loaded"], "Wait for the rebuilt project DLL"
        types = json.dumps(self.ctl("component", "types"))
        assert "ComponentCombatPalette" in types, "Wrong/stale project assembly"
        self.allow_mutations = True
        self.ctl("stop")
        self.ctl("play", "--scene", SCENE)
        self.ctl("pause")
        self.ctl("focus-game")
        self.ctl("wait", "--frames", 3)
        self.steps = 0
        self.ctl("input", "raw", json.dumps([
            {"type": "key_up", "key": key} for key in ["W", "A", "S", "D", "Space"]
        ]))

        # main_camera chooses the first enabled camera, so make selection explicit.
        cameras = [o for o, c in self.scene["components"].items() if CAMERA in c]
        for camera in cameras:
            self.ctl("component", "enable" if camera == cfg["camera"] else "disable",
                     camera, "ComponentCamera")
        for name, rig in PALETTES.items():
            verb = "enable" if name == palette else "disable"
            for field, component in [("sun", "ComponentDirectionalLight"),
                                     ("sky", "ComponentSkyLight"),
                                     ("post", "ComponentGlobalPostProcessVolume")]:
                self.ctl("component", verb, rig[field], component)

        # Remove all ten authored examples from BOTH palettes. The eventual
        # screenshots must contain the real gameplay objects created by firing.
        removed = []
        for obj, components in self.scene["components"].items():
            if components[ID]["name"] in ["Projectile", "Muzzle Flash", "Impact"]:
                self.ctl("object", "delete", obj)
                removed.append(obj)
        assert len(removed) == 18, f"Showcase layout changed: {removed}"
        assert not self.live_rounds()
        assert not any(o["name"] == "Muzzle Flash" for o in self.objects())

        existing_palette = [o for o in self.objects()
                            if self.has_component(o, "ComponentCombatPalette")]
        assert not existing_palette, "Unexpected palette component; avoid first-match ambiguity"
        self.add(cfg["tank"], "ComponentCombatPalette")
        self.put(cfg["tank"], "ComponentCombatPalette", {"palette": {
            "sunny": "Sunny", "sunbaked": "Sunbaked", "moonlit": "Moonlit"}[palette]})

        controller = self.add(cfg["tank"], "ComponentTankController")
        controller.update(self.game_controller)
        controller.update(turret=cfg["turret"], barrel=cfg["barrel"], camera=NIL, crosshair=NIL)
        self.put(cfg["tank"], "ComponentTankController", controller)

        # The reference scene has no colliders or damage targets. Put the
        # existing gameplay hit radius/health on the visible red turret head:
        # its center is at shell height, unlike the tank's ground-level root.
        target = self.target_head(cfg["target"])
        target_component = self.add(target, "ComponentProjectileTarget")
        target_component["hit_radius"] = 1.25
        self.put(target, "ComponentProjectileTarget", target_component)
        target_health = self.add(target, "ComponentHealth")

        before = {"palette": palette, "config": cfg, "removed_static_roots": removed,
                  "controller": controller, "target": target, "target_health": target_health,
                  "camera": self.ctl("object", "get", cfg["camera"]),
                  "camera_settings": self.get(cfg["camera"], "ComponentCamera"),
                  "reference_png": f"pr-assets/et-492/final/{palette}_scenario_far.png",
                  "reference_dimensions": [2062, 932],
                  "objects_before_firing": self.objects(), "info": self.ctl("info"),
                  "editor_discovery": self.discovery,
                  "timing_note": "Visual evidence only, not a frame-time benchmark. Another user-owned editor (PID35800) was reported active; concurrent render load must be stated for any separately measured timing."}
        self.snapshot(f"{palette}-setup.json", before)
        self.capture(f"{palette}-before-live-shot-far")

        # shoot uses ActionState.pressed() (held), not a just_pressed edge.
        # KeyDown while paused -> exactly one simulation frame -> KeyUp while
        # paused. Paused particles also do not age, so the first-frame flash
        # remains visible while screenshot requests complete.
        self.ctl("input", "raw", '[{"type":"key_down","key":"Space"}]')
        try:
            self.step()
        finally:
            self.ctl("input", "raw", '[{"type":"key_up","key":"Space"}]')
        after = self.get(cfg["tank"], "ComponentTankController")
        assert after["ammo"] == controller["ammo"] - 1, "The real controller did not fire once"
        rounds = self.live_rounds()
        assert len(rounds) == 1, f"Expected exactly one live gameplay round: {rounds}"
        flash = self.effect("Muzzle Flash")
        assert flash, "Flash absent after the first frame; inspect frame time/cold shader stalls"
        projectile = rounds[0]["id"]
        initial = self.ctl("object", "get", projectile)["world_transform"]["position"]
        self.snapshot(f"{palette}-actual-firing.json", {
            "ammo_before": controller["ammo"], "ammo_after": after["ammo"],
            "projectile": self.ctl("object", "get", projectile),
            "projectile_component": self.get(projectile, "ComponentProjectile"),
            "flash": self.ctl("object", "get", flash["id"]), "simulated_steps": self.steps})
        self.capture(f"{palette}-flash-far")
        if self.args.closeups:
            self.closeup(palette, "flash", self.ctl("object", "get", flash["id"])["world_transform"]["position"])

        for _ in range(self.args.max_steps):
            rounds = self.live_rounds()
            assert rounds, "Round hit/expired before the requested flight distance"
            position = self.ctl("object", "get", projectile)["world_transform"]["position"]
            distance = sum((a - b) ** 2 for a, b in zip(position, initial)) ** 0.5
            if distance >= self.args.flight_distance:
                break
            self.step()
        else:
            raise AssertionError("Flight distance was never reached")
        self.snapshot(f"{palette}-actual-flight.json", {
            "distance": distance, "position": position, "simulated_steps": self.steps,
            "projectile_component": self.get(projectile, "ComponentProjectile")})
        self.capture(f"{palette}-flight-far")
        if self.args.closeups:
            self.closeup(palette, "flight", position)

        for _ in range(self.args.max_steps):
            impact = self.effect("Tank Impact Sparks")
            if impact:
                break
            self.step()
        else:
            raise AssertionError("No actual hit on the red turret head")
        health_after = self.get(target, "ComponentHealth")
        assert health_after["current_health"] < target_health["current_health"], "Impact did not deal damage"
        assert not self.live_rounds(), "The hit should consume the round"
        impact_object = self.ctl("object", "get", impact["id"])
        self.snapshot(f"{palette}-actual-impact.json", {
            "target": target, "health_before": target_health, "health_after": health_after,
            "impact": impact_object, "simulated_steps": self.steps,
            "lifetime": self.get(impact["id"], "ComponentCombatEffectLifetime")})
        self.capture(f"{palette}-impact-far")
        if self.args.closeups:
            self.closeup(palette, "impact", impact_object["world_transform"]["position"])
        self.step()
        if self.effect("Tank Impact Sparks"):
            self.capture(f"{palette}-impact-spread-far")
            if self.args.closeups:
                self.closeup(palette, "impact-spread", impact_object["world_transform"]["position"])
        previous_age = 1
        for age in [3, 6, 12]:
            for _ in range(age - previous_age):
                self.step()
            previous_age = age
            self.capture(f"{palette}-impact-age{age}-far")
            if self.args.closeups:
                self.closeup(palette, f"impact-age{age}", impact_object["world_transform"]["position"])
            current = self.effect("Tank Impact Sparks")
            self.snapshot(f"{palette}-impact-age{age}.json", {
                "simulated_steps": self.steps,
                "lifetime": self.get(current["id"], "ComponentCombatEffectLifetime") if current else None})
        self.ctl("step", "--frames", 60)
        remaining = [o for o in self.objects()
                     if self.has_component(o, "ComponentCombatEffectLifetime")]
        assert not remaining, f"Combat transients failed to drain: {remaining}"
        self.snapshot(f"{palette}-cleanup.json", {"extra_steps": 60, "transients": remaining})
        print(json.dumps({"palette": palette, "output": str(self.out), "steps": self.steps,
                          "projectile_speed": controller["projectile_speed"],
                          "ammo_after_shot": after["ammo"], "health_after_hit": health_after["current_health"]}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ctl", default="G:/cx497/target/debug/calyx_ctl.exe")
    parser.add_argument("--palette", choices=["sunny", "sunbaked", "moonlit", "both", "all"], default="all")
    parser.add_argument("--closeups", action="store_true")
    parser.add_argument("--flight-distance", type=float, default=7.5,
                        help="Metres of actual flight before the paused flight capture (default7.5)")
    parser.add_argument("--max-steps", type=int, default=180)
    args = parser.parse_args()
    if args.flight_distance <= 0:
        parser.error("--flight-distance must be positive")
    harness = Harness(args)
    try:
        palettes = ["moonlit", "sunny", "sunbaked"] if args.palette == "all" else (["sunny", "sunbaked"] if args.palette == "both" else [args.palette])
        for palette in palettes:
            harness.run_palette(palette)
    finally:
        # Never leave injected gameplay input held, including on an assertion.
        if harness.allow_mutations:
            try:
                harness.ctl("input", "raw", '[{"type":"key_up","key":"Space"}]')
                harness.ctl("pause")
            except (RuntimeError, subprocess.TimeoutExpired):
                pass
    print(f"Evidence written to {harness.out}; editor left paused, no scene saved.")


if __name__ == "__main__":
    main()
