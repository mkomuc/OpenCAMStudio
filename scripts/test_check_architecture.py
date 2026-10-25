"""Exercise forbidden edges that ordinary compilation would accept."""

import unittest

from check_architecture import violations


def package(name, dependencies=(), features=None):
    return {
        "id": name, "name": name, "dependencies": list(dependencies),
        "features": features or {},
    }


def dependency(name, **overrides):
    return {"name": name, "kind": None, "optional": False, **overrides}


def check(*packages):
    return violations({"workspace_members": [p["id"] for p in packages], "packages": packages})


class ArchitectureTests(unittest.TestCase):
    def test_supported_direction(self):
        self.assertEqual(check(package("cam-model", [dependency("cam-geo")]), package("cam-geo")), [])

    def test_reverse_direction_and_renamed_dependency(self):
        errors = check(package("cam-geo", [dependency("cam-model", rename="model")]), package("cam-model"))
        self.assertTrue(any("forbidden workspace dependency" in e for e in errors))

    def test_optional_target_specific_build_edge_still_counts(self):
        errors = check(package("cam-post", [dependency("cam-kernel", kind="build", optional=True,
                                                      target="cfg(windows)")]), package("cam-kernel"))
        self.assertTrue(any("forbidden workspace dependency" in e for e in errors))

    def test_integration_tests_can_cross_layers(self):
        self.assertEqual(check(package("cam-import", [dependency("cam-post", kind="dev")]),
                               package("cam-post")), [])

    def test_ui_cannot_enter_core_even_as_optional_dependency(self):
        self.assertTrue(check(package("cam-toolpath", [dependency("wgpu", optional=True)])))

    def test_ui_must_remain_optional_in_shell(self):
        self.assertTrue(check(package("cam-app", [dependency("iced")])))
        self.assertEqual(check(package("cam-app", [dependency("iced", optional=True)])), [])

    def test_default_features_cannot_enable_gui_or_native_backend(self):
        for name, feature in (("cam-app", "gui"), ("cam-render", "gpu"),
                              ("cam-import", "occt"), ("cam-kernel", "occt")):
            with self.subTest(name=name):
                self.assertTrue(check(package(name, features={"default": [feature]})))

    def test_new_workspace_crate_needs_policy(self):
        self.assertTrue(check(package("cam-new")))

    def test_nonworkspace_packages_are_not_subject_to_layer_policy(self):
        self.assertEqual(violations({"workspace_members": [], "packages": [package("external")]}), [])


if __name__ == "__main__":
    unittest.main()
