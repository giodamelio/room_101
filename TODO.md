# TODO

 - [x] Allow a node to announce it is deleting one of it's secrets by sending a signed message. Don't allow a node to send a delete for messages that belong to any other node
 - [x] Allow the WebUI to "share" a secret that that node owns to other nodes, but creating copies of it.
 - [x] Add copy buttons for places that node ids and hashs are displayed
 - [ ] Add support for secret versions. Update schema to handle storing multiple versions of the same secret and making sure to actually use the last one. Maybe something we can use DB views for.
 - [ ] Add button to peer to view all secrets for that peer (using a new filter queryparam on the list page)
 - [ ] Sync secrets that are for the current node into systemd encrypted secrets. Only do it when the secret is updated.
