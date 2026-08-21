import unittest

from scripts.public_api import normalized_api, publishable_libraries, snapshot_diff


class PublicApiTest(unittest.TestCase):
    def test_publishable_libraries_filters_and_sorts(self):
        metadata = {
            "workspace_members": ["facade", "private", "binary", "external-not-in-packages"],
            "packages": [
                {
                    "id": "private",
                    "name": "private-package",
                    "publish": [],
                    "targets": [{"kind": ["lib"]}],
                },
                {
                    "id": "facade",
                    "name": "a-facade",
                    "publish": None,
                    "targets": [{"kind": ["lib"]}],
                },
                {
                    "id": "binary",
                    "name": "binary-only",
                    "publish": None,
                    "targets": [{"kind": ["bin"]}],
                },
                {
                    "id": "outside",
                    "name": "outside",
                    "publish": None,
                    "targets": [{"kind": ["lib"]}],
                },
                {
                    "id": "z-pattern",
                    "name": "z-pattern",
                    "publish": ["crates-io"],
                    "targets": [{"kind": ["lib"]}],
                },
            ],
        }
        metadata["workspace_members"].append("z-pattern")

        self.assertEqual(publishable_libraries(metadata), ["a-facade", "z-pattern"])

    def test_normalized_api_has_one_trailing_newline(self):
        self.assertEqual(normalized_api("one\n\n"), "one\n")
        self.assertEqual(normalized_api(""), "\n")

    def test_snapshot_diff_names_the_snapshot_and_generated_output(self):
        diff = snapshot_diff("tower-resilience-core", "old\n", "new\n")

        self.assertIn("--- docs/public-api/tower-resilience-core.txt", diff)
        self.assertIn("+++ generated:tower-resilience-core", diff)
        self.assertIn("-old", diff)
        self.assertIn("+new", diff)


if __name__ == "__main__":
    unittest.main()
