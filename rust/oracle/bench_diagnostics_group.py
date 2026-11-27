import json

from diagnostics_group import group

CASES = [
    {
        "relative_path": "src/example.py",
        "entries": [
            {
                "name_path": "<file>",
                "diagnostic": {
                    "message": "Unexpected argument",
                    "range": {
                        "start": {"line": 4, "character": 8},
                        "end": {"line": 4, "character": 21},
                    },
                    "severity": 2,
                    "source": "Pyright",
                },
            },
            {
                "name_path": "MyClass/my_method",
                "diagnostic": {
                    "message": "Type mismatch",
                    "range": {
                        "start": {"line": 11, "character": 4},
                        "end": {"line": 11, "character": 18},
                    },
                    "severity": 1,
                    "code": 2322,
                    "source": "typescript",
                },
            },
        ],
    },
]


def _run() -> None:
    result = group(CASES[0]["relative_path"], CASES[0]["entries"])
    assert result["src/example.py"]["Warning"]["<file>"][0]["message"] == "Unexpected argument"
    assert result["src/example.py"]["Error"]["MyClass/my_method"][0]["code"] == 2322


if __name__ == "__main__":
    _run()
    print("diagnostics_group oracle ok")
