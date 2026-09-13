"""Pin local-library dependency discovery and forbidden edges."""
import json
import subprocess
import unittest
from scripts.check_dependency_policy import cargo_executable, invalid_edges


def metadata(names, edges=(), external=()):
    packages = [{"id": "local:" + name, "name": name, "dependencies": [
        {"name": dependency, "kind": None} for owner, dependency in edges if owner == name
    ]} for name in names]
    packages += [{"id": "external:" + name, "name": name, "dependencies": []} for name in external]
    return {"workspace_members": ["local:" + name for name in names], "packages": packages}


class DependencyPolicyTests(unittest.TestCase):
    def test_real_workspace(self):
        data = json.loads(subprocess.check_output([
            cargo_executable(), "metadata", "--no-deps", "--format-version", "1", "--locked"
        ], text=True))
        self.assertEqual(invalid_edges(data), [])

    def test_new_library_allowed_edges(self):
        names = ["spherra", "spherra-codec", "spherra-format", "spherra-domain", "spherra-bench"]
        edges = [("spherra", name) for name in names[1:4]] + [("spherra-bench", "spherra")]
        self.assertEqual(invalid_edges(metadata(names, edges)), [])

    def test_forbidden_edges(self):
        for edge in [("spherra-codec", "spherra"), ("spherra-format", "spherra-codec"),
                     ("spherra", "spherra-testkit"), ("spherra", "spherra-bench")]:
            with self.subTest(edge=edge):
                self.assertEqual(invalid_edges(metadata(list(edge), [edge])), [edge])

    def test_nonworkspace_spherra_is_not_discovered(self):
        self.assertEqual(invalid_edges(metadata(["spherra-codec"],
            [("spherra-codec", "spherra")], external=["spherra"])), [])

    def test_dev_edges_remain_outside_the_policy(self):
        data = metadata(["spherra-codec", "spherra"], [("spherra-codec", "spherra")])
        data["packages"][0]["dependencies"][0]["kind"] = "dev"
        self.assertEqual(invalid_edges(data), [])


if __name__ == "__main__":
    unittest.main()
