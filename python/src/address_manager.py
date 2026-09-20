from typing import List, Dict, Any
from database import database


class AddressManager:

    def _read_peers(self) -> List[Dict[str, Any]]:
        # Read peers from database
        # Named columns: `ip` is a native inet now and the table gained a
        # primary key, so neither the order nor the count is what it was.
        result = database.query("SELECT ip, services, port FROM addr")
        # f-string on the ip because psycopg may hand back either a str or an
        # ipaddress object depending on the adapters loaded; both render the
        # same way and the API has always emitted a string.
        return [{"ip": f"{x[0]}", "services": x[1], "port": x[2]} for x in result]

    def get_peers(self) -> Dict[str, List[Dict[str, Any]]]:
        """ Return a dictionary of addresses"""
        return {
            "peers": self._read_peers(),
        }


address_manager = AddressManager()
