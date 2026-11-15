import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from airglow import config, transactions


class InterruptedSave(BaseException):
    pass


class TransactionRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = config.Store(self.temp.name)
        self.root = Path(self.temp.name)

    def test_process_death_between_profile_and_selection_writes_restores_both_on_restart(self):
        before = {p.name: p.read_bytes() for p in self.root.glob("*.json")}
        presets = self.store.get_presets()
        presets[0]["name"] = "Interrupted name"
        state = {**self.store.state, "active_preset": presets[0]["id"]}
        write = self.store._write

        def crash(name, value):
            if name == "state.json":
                raise InterruptedSave()
            write(name, value)

        with mock.patch.object(self.store, "_write", side_effect=crash), self.assertRaises(InterruptedSave):
            self.store._commit_files([("presets.json", presets), ("state.json", state)])
        config.Store(self.root)
        self.assertEqual({p.name: p.read_bytes() for p in self.root.glob("*.json")}, before)

    def test_recovery_does_not_overwrite_an_intervening_external_edit(self):
        write = self.store._write

        def crash(name, value):
            if name == "state.json":
                raise InterruptedSave()
            write(name, value)

        with mock.patch.object(self.store, "_write", side_effect=crash), self.assertRaises(InterruptedSave):
            self.store._commit_files([("presets.json", []), ("state.json", self.store.state)])
        external = b'[{"id":"external","name":"keep this edit"}]'
        (self.root / "presets.json").write_bytes(external)
        with self.assertRaisesRegex(ValueError, "recover|Recovery"):
            config.Store(self.root)
        self.assertEqual((self.root / "presets.json").read_bytes(), external)

    def test_committed_group_survives_death_before_journal_cleanup(self):
        presets = self.store.get_presets()
        presets[0]["name"] = "Committed name"
        state = {**self.store.state, "active_preset": presets[0]["id"]}
        unlink = Path.unlink

        def crash_cleanup(path, *args, **kwargs):
            if path.name == ".transaction.json":
                raise InterruptedSave()
            return unlink(path, *args, **kwargs)

        with mock.patch.object(Path, "unlink", crash_cleanup), self.assertRaises(InterruptedSave):
            self.store._commit_files([("presets.json", presets), ("state.json", state)])
        recovered = config.Store(self.root)
        self.assertEqual(recovered.get_presets()[0]["name"], "Committed name")
        self.assertEqual(recovered.state["active_preset"], presets[0]["id"])
        self.assertFalse((self.root / ".transaction.json").exists())

    def test_corrupt_or_path_traversing_journal_blocks_recovery_without_touching_files(self):
        original = (self.root / "config.json").read_bytes()
        for text in ("{broken", json.dumps({"version": 1, "committed": False,
                                           "files": {"../outside.json": {"before": None, "after": "{}"}}})):
            (self.root / ".transaction.json").write_text(text, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "recover|Recovery"):
                config.Store(self.root)
            self.assertEqual((self.root / "config.json").read_bytes(), original)

    def test_migration_is_recoverable_before_publishing_the_new_schema(self):
        legacy = json.loads((self.root / "config.json").read_text())
        legacy["schema"] = 0
        (self.root / "config.json").write_text(json.dumps(legacy), encoding="utf-8")
        before = {p.name: p.read_bytes() for p in self.root.glob("*.json")}
        write = config.Store._write

        def crash(store, name, value):
            if name == "presets.json":
                raise InterruptedSave()
            write(store, name, value)

        with mock.patch.object(config.Store, "_write", crash), self.assertRaises(InterruptedSave):
            config.Store(self.root)
        transactions.recover(self.root)
        self.assertEqual({p.name: p.read_bytes() for p in self.root.glob("*.json")}, before)
        self.assertEqual(config.Store(self.root).schema, config.SCHEMA_VERSION)

    def test_edit_between_recovery_validation_and_apply_is_preserved(self):
        write = self.store._write

        def crash(name, value):
            if name == "state.json":
                raise InterruptedSave()
            write(name, value)

        with mock.patch.object(self.store, "_write", side_effect=crash), self.assertRaises(InterruptedSave):
            self.store._commit_files([("presets.json", []), ("state.json", self.store.state)])
        read, count = transactions._read, 0
        external = '[{"id":"editor","name":"preserve"}]'

        def edit(path):
            nonlocal count
            if path.name == "presets.json":
                count += 1
                if count == 2:
                    path.write_text(external, encoding="utf-8")
            return read(path)

        with mock.patch.object(transactions, "_read", edit), self.assertRaisesRegex(ValueError, "recover|Recovery"):
            transactions.recover(self.root)
        self.assertEqual((self.root / "presets.json").read_text(), external)
