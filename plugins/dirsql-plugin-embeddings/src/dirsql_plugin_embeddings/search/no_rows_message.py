def no_rows_message(glob, matched, root):
    if not matched:
        return f"no files matched {glob!r} (searched from {root})"
    return (
        f"{glob!r} matched {matched} file(s), but none had text content to"
        f" embed -- unreadable or not valid UTF-8 (searched from {root})"
    )
