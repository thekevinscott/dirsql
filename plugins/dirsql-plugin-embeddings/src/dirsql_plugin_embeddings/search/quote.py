def quote(text):
    escaped = text.replace("'", "''")
    return f"'{escaped}'"
