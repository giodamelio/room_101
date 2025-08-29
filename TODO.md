# TODO

 - [x] Allow a node to announce it is deleting one of it's secrets by sending a signed message. Don't allow a node to send a delete for messages that belong to any other node
 - [x] Allow the WebUI to "share" a secret that that node owns to other nodes, but creating copies of it.
 - [x] Add copy buttons for places that node ids and hashs are displayed
 - [x] Add button to peer to view all secrets for that peer (using a new filter queryparam on the list page)
 - [ ] Create comprehensive node details page showing all peer info and embedded secrets list:
   - Add `/peers/:node_id` route and handler
   - Create `tmpl_peer_detail()` template with peer metadata, connection info, and embedded secrets
   - Make node IDs in peer list clickable links to detail page
   - Keep existing "View Secrets" button as quick-access option
 - [ ] Sync secrets that are for the current node into systemd encrypted secrets. Only do it when the secret is updated.
 - [ ] Add support for secret versions. Update schema to handle storing multiple versions of the same secret and making sure to actually use the last one. Maybe something we can use DB views for.
