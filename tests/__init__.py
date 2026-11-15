"""Isolate the unit-test process before any Airglow module is imported."""

from .isolation import isolate

isolate()
