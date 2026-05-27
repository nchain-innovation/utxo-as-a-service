import os
import sys

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC_DIR = os.path.join(PROJECT_ROOT, "python", "src")
DATA_DIR = os.path.join(PROJECT_ROOT, "data")
PYTHON_DATA_LINK = os.path.join(PROJECT_ROOT, "python", "data")

sys.path.insert(0, SRC_DIR)

# rest_api loads ../data/uaasr.toml relative to python/src (same layout as Docker).
if not os.path.lexists(PYTHON_DATA_LINK):
    os.symlink(DATA_DIR, PYTHON_DATA_LINK)

os.chdir(SRC_DIR)
