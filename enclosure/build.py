"""freecadcmd entry point: build the enclosure from params.toml and write
the FCStd, STEP and STL files to enclosure/build (see box.main).

freecadcmd runs a script file as a module named after the file, never as
__main__, does not put the script's folder on sys.path, and drops buffered
stdout when the script calls sys.exit, hence the path insert and the flush.
"""

import os
import sys
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import box  # noqa: E402

try:
    box.main()
    code = 0
except box.ParamError as e:
    print(f"error: {e}")
    code = 1
except Exception:
    traceback.print_exc(file=sys.stdout)
    code = 1
sys.stdout.flush()
sys.exit(code)
