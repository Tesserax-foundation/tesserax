"""Errors of the SDK."""


class TesseraxError(Exception):
    """Base class of the errors of the SDK."""


class InvalidAddress(TesseraxError, ValueError):
    """An address is not a Tesserax address."""


class InvalidAmount(TesseraxError, ValueError):
    """An amount is not a valid amount of TESS."""


class InvalidKey(TesseraxError, ValueError):
    """A secret URI or seed is invalid."""


class InvalidInput(TesseraxError, ValueError):
    """An input does not fit the limits of the chain."""


class TransactionFailed(TesseraxError):
    """The chain refused or failed a transaction. `error` is the name of the chain error, such as
    `Agents::BudgetExceeded`, when known."""

    def __init__(self, message: str, error: str | None = None):
        super().__init__(message)
        self.error = error
