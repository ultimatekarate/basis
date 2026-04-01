# Laboratory layer — can import from dictionary, NOT from hands

from src.models import types    # OK: laboratory -> dictionary
from src.api import server      # VIOLATION: laboratory -> hands
import os                       # VIOLATION: external dep 'os' forbidden in laboratory
import re                       # OK: 're' is in allow list

def validate(data):
    pass
