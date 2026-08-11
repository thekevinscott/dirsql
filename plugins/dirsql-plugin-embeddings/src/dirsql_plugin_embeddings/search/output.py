def format_rows(rows):
    return [f"{row['path']}\t{row['distance']:.6f}" for row in rows]
