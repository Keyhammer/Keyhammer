# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Robson Trasel
"""Type stubs for the compiled module (re-exported by `keyhammer`)."""

from typing import ClassVar, Iterable, Optional, Tuple, final

class KeyhammerError(ValueError): ...
class BuildError(KeyhammerError): ...
class SearchError(KeyhammerError): ...
class QueryTooLongError(SearchError): ...
class BudgetTooLargeError(SearchError): ...

@final
class Ranking:
    COARSE: ClassVar[Ranking]
    EXACT: ClassVar[Ranking]
    def __eq__(self, value: object, /) -> bool: ...
    def __hash__(self) -> int: ...

@final
class SearchConfig:
    k: int
    budget: int
    ranking: Ranking
    tsb: bool
    max_nodes: int
    def __new__(
        cls,
        k: int = 10,
        budget: int = 32,
        ranking: Ranking = ...,
        tsb: bool = False,
        max_nodes: int = 100_000,
    ) -> SearchConfig: ...
    @staticmethod
    def high_recall() -> SearchConfig: ...

@final
class Hit:
    term: str
    cost: int
    weight: int
    index: int
    def __eq__(self, value: object, /) -> bool: ...
    def __hash__(self) -> int: ...

@final
class SearchResult:
    hits: list[Hit]
    nodes_expanded: int
    truncated: bool
    def __len__(self) -> int: ...

@final
class Index:
    def __new__(cls, items: Iterable[Tuple[str, int]]) -> Index: ...
    def __len__(self) -> int: ...
    def search(
        self,
        query: str,
        k: Optional[int] = None,
        budget: Optional[int] = None,
        ranking: Optional[Ranking] = None,
        *,
        config: Optional[SearchConfig] = None,
    ) -> SearchResult: ...
