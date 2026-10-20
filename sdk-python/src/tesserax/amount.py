"""Amounts of TESS, with 12 decimals."""

from __future__ import annotations

from decimal import Decimal, InvalidOperation

from .errors import InvalidAmount

DECIMALS = 12
PLANCK_PER_TESS = 10**DECIMALS


class Amount:
    """An amount of TESS, held in planck. Printed as a decimal string of TESS, such as "1.5"."""

    __slots__ = ("planck",)

    def __init__(self, planck: int):
        if not isinstance(planck, int) or planck < 0 or planck >= 1 << 128:
            raise InvalidAmount(f"{planck!r} is not an amount in planck")
        self.planck = planck

    @classmethod
    def from_tess(cls, value: str | int | Decimal) -> "Amount":
        """Parse an amount of TESS such as "1.5", 2 or Decimal("0.25")."""
        if isinstance(value, float):
            raise InvalidAmount("use a string or a Decimal, not a float")
        try:
            tess = Decimal(str(value).strip())
        except InvalidOperation:
            raise InvalidAmount(f"{value!r} is not an amount of TESS") from None
        if not tess.is_finite() or tess < 0:
            raise InvalidAmount(f"{value!r} is not an amount of TESS")
        planck = tess * PLANCK_PER_TESS
        if planck != planck.to_integral_value():
            raise InvalidAmount(f"{value!r} has more than {DECIMALS} decimals")
        return cls(int(planck))

    def __str__(self) -> str:
        whole, fraction = divmod(self.planck, PLANCK_PER_TESS)
        if fraction == 0:
            return str(whole)
        return f"{whole}.{fraction:012d}".rstrip("0")

    def __repr__(self) -> str:
        return f"Amount('{self}')"

    def __eq__(self, other) -> bool:
        return isinstance(other, Amount) and other.planck == self.planck

    def __lt__(self, other: "Amount") -> bool:
        return self.planck < other.planck

    def __le__(self, other: "Amount") -> bool:
        return self.planck <= other.planck

    def __hash__(self) -> int:
        return hash(self.planck)

    def __add__(self, other: "Amount") -> "Amount":
        return Amount(self.planck + other.planck)

    def __sub__(self, other: "Amount") -> "Amount":
        return Amount(self.planck - other.planck)
