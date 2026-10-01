"""RFC3339 timestamps with integer nanosecond comparison, without float epochs."""

import re
from datetime import date, timedelta

_TIMESTAMP = re.compile(
    # Rust time accepts any single-byte date/time separator and emits canonical T.
    r"([0-9]{4})-([0-9]{2})-([0-9]{2})[\x00-\x7f]([0-9]{2}):([0-9]{2}):([0-9]{2})"
    + r"(?:\.([0-9]+))?([Zz]|[+-][0-9]{2}:[0-9]{2})\Z"
)


def parse_timestamp(value: str) -> tuple[str, int]:
    match = _TIMESTAMP.fullmatch(value)
    if match is None:
        raise ValueError("expected RFC3339 timestamp")
    year, month, day, hour, minute, second = (int(match[i]) for i in range(1, 7))
    if year == 0:
        # Gregorian year zero is supported by Rust time, but not datetime.date.
        ordinal = date(400, month, day).toordinal() - 146097
    else:
        ordinal = date(year, month, day).toordinal()
    if hour > 23 or minute > 59 or second > 60:
        raise ValueError("invalid timestamp time")
    zone = match[8]
    offset = 0
    if zone not in ("Z", "z"):
        hours, minutes = int(zone[1:3]), int(zone[4:6])
        if hours > 23 or minutes > 59:
            raise ValueError("invalid timestamp offset")
        offset = (hours * 3600 + minutes * 60) * (1 if zone[0] == "+" else -1)
    fraction = int(((match[7] or "") + "000000000")[:9])
    # time's RFC3339 parser represents leap seconds as the final ns of second 59.
    if second == 60:
        second, fraction = 59, 999999999
        utc_day, utc_second = divmod(
            ordinal * 86400 + hour * 3600 + minute * 60 + 59 - offset, 86400
        )
        utc_date = date.fromordinal((utc_day - 1) % 146097 + 1)
        if utc_second != 86399 or (utc_date + timedelta(days=1)).day != 1:
            raise ValueError("leap second must end a UTC month")
    nanos = (ordinal * 86400 + hour * 3600 + minute * 60 + second - offset) * 10**9 + fraction
    subseconds = f".{fraction:09d}".rstrip("0") if fraction else ""
    suffix = "Z" if offset == 0 else zone
    canonical = (
        f"{year:04d}-{month:02d}-{day:02d}T{hour:02d}:{minute:02d}:{second:02d}{subseconds}{suffix}"
    )
    return canonical, nanos


def timestamp_nanos(value: str) -> int:
    return parse_timestamp(value)[1]
