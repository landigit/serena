"""Oracle for astlens name-path matching.

Thin functions over Serena's real implementation so rust-anything can record and
gate the exact behavior. This file is the reference; the Rust port lives in
rust/name-path/src/port.rs.
"""

from __future__ import annotations

from serena.symbol import NamePathComponent, NamePathMatcher

# Patterns and name paths exercised by astlens's read_symbol/find tools.
CORPUS: list[tuple[str, list[str], bool]] = [
    ("foo", ["foo"], False),
    ("foo/", ["foo"], False),
    ("foo", ["bar", "foo"], False),
    ("foo", ["foobar"], False),
    ("/foo", ["foo"], False),
    ("/foo", ["bar", "foo"], False),
    ("foo", ["foobar"], True),
    ("foo", ["bar", "my_foobar_func"], True),
    ("/foo", ["barfoobaz"], True),
    ("bar/foo", ["bar", "foo"], False),
    ("bar/foo", ["mod", "bar", "foo"], False),
    ("bar/foo", ["bar", "foobar"], True),
    ("ClassA/method", ["ClassA", "method"], False),
    ("ClassA/method[1]", ["ClassA", "method"], False),
    ("ClassA/method[1]", ["ClassA", "method", "inner"], False),
    ("inner", ["ClassA", "method", "inner"], False),
    ("/ClassA/method/inner", ["ClassA", "method", "inner"], False),
]


def match_name_path(pattern: str, parts: list[str], substring: bool) -> bool:
    """Match a name-path pattern against a symbol's name path components."""
    components = [NamePathComponent(part) for part in parts]
    matcher = NamePathMatcher(pattern, substring)
    return matcher.matches_reversed_components(reversed(components))


def pattern_components(pattern: str) -> list[list[str | int | None]]:
    """Parse a pattern into [name, overload_idx] components."""
    matcher = NamePathMatcher(pattern, False)
    return [[component.name, component.overload_idx] for component in matcher._components]


OVERLOADED_CORPUS: list[tuple[str, list[list[str | int | None]], bool]] = [
    ("Calc/add[0]", [["Calc", None], ["add", 0]], False),
    ("Calc/add[1]", [["Calc", None], ["add", 0]], False),
    ("Calc/add[1]", [["Calc", None], ["add", 1]], False),
    ("Calc/add", [["Calc", None], ["add", 1]], False),
    ("/Calc/add[1]", [["Calc", None], ["add", 1]], False),
    ("Calc/ad[1]", [["Calc", None], ["add", 1]], True),
]


def match_name_path_overloaded(pattern: str, components: list[list[str | int | None]], substring: bool) -> bool:
    """Match a name-path pattern against [name, overload_idx] components."""
    parsed = [NamePathComponent(name, overload_idx) for name, overload_idx in components]
    matcher = NamePathMatcher(pattern, substring)
    return matcher.matches_reversed_components(reversed(parsed))


