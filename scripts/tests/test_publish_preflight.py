from __future__ import annotations

import json
from pathlib import Path
import unittest

from scripts import publish_preflight


class PublishPreflightTests(unittest.TestCase):
    def setUp(self) -> None:
        fixture = Path(__file__).parent / "fixtures" / "invalid_dependency_metadata.json"
        self.metadata = json.loads(fixture.read_text(encoding="utf-8"))

    def test_publish_false_package_is_skipped(self) -> None:
        packages = publish_preflight.publishable_packages(self.metadata)
        names = {package["name"] for package in packages}
        self.assertEqual(names, {"internal", "broken"})

    def test_path_only_dependency_fixture_fails_validation(self) -> None:
        packages = publish_preflight.publishable_packages(self.metadata)
        errors = publish_preflight.validate_dependencies(packages)
        self.assertTrue(any("forbidden path-only dependency" in error for error in errors), errors)
        self.assertTrue(
            any("expected the synchronized version 1.0.0" in error for error in errors),
            errors,
        )

    def test_dependency_order_places_dependency_first(self) -> None:
        packages = publish_preflight.publishable_packages(self.metadata)
        names = [package["name"] for package in publish_preflight.publication_order(packages)]
        self.assertEqual(names, ["internal", "broken"])


if __name__ == "__main__":
    unittest.main()
