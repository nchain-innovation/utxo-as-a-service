from typing import List, Dict, Any
from database import database


class AddressManager:

    def _read_peers(self) -> List[Dict[str, Any]]:
        # Read peers from database
        result = database.query("SELECT * FROM addr")
        return [{"ip": f"{x[0]}", "services": x[1], "port": x[2]} for x in result]

    def get_peers(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of addresses"""
        return {
            "peers": self._read_peers(),
        }


address_manager = AddressManager()
