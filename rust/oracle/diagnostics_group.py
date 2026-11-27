"""Oracle for astlens diagnostics grouping (ported from serena's GroupedDiagnostics).

Pure grouping logic over diagnostic entries: returns the nested
file -> severity -> name_path -> [diagnostic outputs] structure.
"""

SEVERITY_NAMES = {1: "Error", 2: "Warning", 3: "Information", 4: "Hint"}


def severity_name(severity: int | None) -> str:
    if severity is None:
        return "Unknown"
    return SEVERITY_NAMES.get(severity, f"Severity_{severity}")


def diagnostic_output(diagnostic: dict) -> dict:
    result: dict = {"message": diagnostic.get("message", ""), "range": diagnostic.get("range", {})}
    if "code" in diagnostic:
        result["code"] = diagnostic["code"]
    if "source" in diagnostic:
        result["source"] = diagnostic["source"]
    return result


def group(relative_path: str, entries: list[dict]) -> dict:
    """group(relative_path, [{"name_path": ..., "diagnostic": {...}}, ...]) -> grouped dict."""
    grouped: dict = {}
    file_map = grouped.setdefault(relative_path, {})
    for entry in entries:
        name_path = entry.get("name_path", "<file>")
        diagnostic = entry.get("diagnostic", {})
        severity = severity_name(diagnostic.get("severity"))
        severity_map = file_map.setdefault(severity, {})
        severity_map.setdefault(name_path, []).append(diagnostic_output(diagnostic))
    return grouped
