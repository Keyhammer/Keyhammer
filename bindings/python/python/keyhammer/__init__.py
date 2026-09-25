# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Robson Trasel
"""Typo-tolerant top-k search with keyboard-aware edit costs.

Prototype: the API may change. Terms and queries are Unicode text, normalised
identically (case and diacritics folded by default, see the README); there is
no add/remove/export yet.
"""

from keyhammer._keyhammer import (
    BudgetTooLargeError,
    BuildError,
    Hit,
    Index,
    KeyhammerError,
    QueryTooLongError,
    Ranking,
    SearchConfig,
    SearchError,
    SearchResult,
)

__all__ = [
    "BudgetTooLargeError",
    "BuildError",
    "Hit",
    "Index",
    "KeyhammerError",
    "QueryTooLongError",
    "Ranking",
    "SearchConfig",
    "SearchError",
    "SearchResult",
]
