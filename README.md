# Bors
IoT Server client spawner/management for collaborative-IoT.

This server is specifically for abstracting communications between IoT servers and the main server(Merlin). This server is often referred to as the integrations
server, because each supported IOT server must have the matching integration implementation to abstract API data into models that the general server
can understand. 

Extra: Some data needs to be filtered and could be considered irrelevant for our platform so it needs to be filtered at this level.
