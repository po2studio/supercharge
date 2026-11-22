"""Fees charged to guests for changes to a stay."""

# Guests who arrive after 22:00 without telling the property pay a late check-in fee.
LATE_CHECKIN_FEE_ISK = 4900

# Cancellation inside 48 hours is charged at the first night's price.
CANCELLATION_HOURS = 48


def late_checkin_fee(arrival_hour: int, told_property: bool) -> int:
    """The fee for arriving at `arrival_hour` (24h clock)."""
    if arrival_hour >= 22 and not told_property:
        return LATE_CHECKIN_FEE_ISK
    return 0
