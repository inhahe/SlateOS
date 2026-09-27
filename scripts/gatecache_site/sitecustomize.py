"""Gate-cache bootstrap: trace this Python process when a cached gate runs it.

`gatecache_trace.py` puts this directory first on `PYTHONPATH` for every
Python process a traced gate starts, and `gate-cache.py` does the same for the
gate itself, so Python's `site` module imports this file before the program's
first line. It does nothing unless `GATE_CACHE_TRACE_DIR` names a trace
directory; the directory holds only this file, so it shadows no other module.

The tracer is loaded by path, not by adding `scripts/` to `sys.path`, so the
traced program's own imports resolve exactly as they would untraced. Any
other `sitecustomize` further along the path is then run as well, since
putting this one first hides it.
"""

import importlib.util
import os
import sys


def _load_tracer() -> None:
    here = os.path.dirname(os.path.abspath(__file__))
    path = os.path.join(os.path.dirname(here), "gatecache_trace.py")
    spec = importlib.util.spec_from_file_location("gatecache_trace", path)
    if spec is None or spec.loader is None:
        return
    module = importlib.util.module_from_spec(spec)
    sys.modules["gatecache_trace"] = module
    spec.loader.exec_module(module)
    module.install()


def _chain() -> None:
    mine = os.path.normcase(os.path.abspath(__file__))
    for entry in sys.path:
        candidate = os.path.join(entry or os.getcwd(), "sitecustomize.py")
        if os.path.normcase(os.path.abspath(candidate)) == mine:
            continue
        if os.path.isfile(candidate):
            spec = importlib.util.spec_from_file_location("_chained_sitecustomize",
                                                          candidate)
            if spec is not None and spec.loader is not None:
                spec.loader.exec_module(importlib.util.module_from_spec(spec))
            return


if os.environ.get("GATE_CACHE_TRACE_DIR"):
    _load_tracer()
_chain()
