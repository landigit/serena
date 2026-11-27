"""Workload for rust-anything profile/go: exercise Serena's name-path matcher."""

from name_path import CORPUS, match_name_path, pattern_components


def bench(iterations: int = 200_000) -> None:
    for _ in range(iterations):
        for pattern, parts, substring in CORPUS:
            match_name_path(pattern, parts, substring)
        for pattern, _, _ in CORPUS:
            pattern_components(pattern)


if __name__ == "__main__":
    bench()
    assert match_name_path("bar/foo", ["mod", "bar", "foo"], False)
    assert not match_name_path("/foo", ["bar", "foo"], False)
    assert match_name_path("foo", ["my_foobar_func"], True)
    assert pattern_components("ClassA/method[1]") == [["ClassA", None], ["method", 1]]
    print("name_path oracle ok")
