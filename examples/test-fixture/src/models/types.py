# Dictionary layer — should NOT import from laboratory or hands

from src.logic import validator  # VIOLATION: dictionary -> laboratory
from src.api import server       # VIOLATION: dictionary -> hands
import os                        # VIOLATION: external dep 'os' forbidden in dictionary
import requests                  # VIOLATION: external dep 'requests' forbidden in dictionary
from dataclasses import dataclass  # OK: 'dataclasses' is in allow list
from typing import NewType

UserId = NewType("UserId", str)
OrderId = NewType("OrderId", str)
EmailAddress = NewType("EmailAddress", str)

@dataclass
class User:
    user_id: UserId
    email: EmailAddress


# VIOLATION: uses raw str where UserId should be used
def get_user_id(user_id: str) -> str:
    return user_id


# OK: uses branded type
def process_user(user_id: UserId) -> None:
    pass


# VIOLATION: uses raw str where EmailAddress should be used
def send_email(email_address: str) -> None:
    pass


# OK: private function, not checked
def _internal_helper(user_id: str) -> str:
    return user_id
