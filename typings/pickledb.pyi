from typing import Iterable
from typing import Union

class PickleDB(object):
    def set(self, key: str, value: str) -> bool: ...
    def get(self, key: str) -> Union[bool, str]: ...
    def dump(self) -> bool: ...
    def getall(self) -> Iterable[str]: ...

def load(location: str, auto_dump: bool, sig: bool = True) -> PickleDB: ...