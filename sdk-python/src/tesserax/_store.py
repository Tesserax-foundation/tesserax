"""JSON state files, written atomically: a crash leaves either the previous or the new content,
never a partial file."""

from __future__ import annotations

import json
import os
from typing import Any, Optional

from .errors import StorageError


def load(path: str | os.PathLike) -> Optional[Any]:
    """The JSON content of the file at `path`, or None if it does not exist."""
    try:
        with open(path, "rb") as file:
            data = file.read()
    except FileNotFoundError:
        return None
    except OSError as error:
        raise StorageError(f"cannot read {path}: {error}") from None
    try:
        return json.loads(data)
    except ValueError as error:
        raise StorageError(f"{path} is not a valid state file: {error}") from None


def save(path: str | os.PathLike, value: Any) -> None:
    """Replace the file at `path` with `value` as JSON, readable by the user only: written to a
    temporary file in the same directory, flushed to disk, then renamed over `path`."""
    path = os.fspath(path)
    directory, name = os.path.split(path)
    temporary = os.path.join(directory, f".{name}.tmp")
    data = json.dumps(value, indent=2, sort_keys=True).encode()
    try:
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | getattr(os, "O_BINARY", 0), 0o600)
        try:
            os.write(fd, data)
            os.fsync(fd)
        finally:
            os.close(fd)
        os.replace(temporary, path)
        if os.name == "posix":
            # Make the rename itself durable.
            dir_fd = os.open(directory or ".", os.O_RDONLY)
            try:
                os.fsync(dir_fd)
            finally:
                os.close(dir_fd)
    except OSError as error:
        raise StorageError(f"cannot write {path}: {error}") from None
