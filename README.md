Headscale auto DNS
----------

This tool is built for a very specific use-case I don't think many people will find useful.
If you do, let me know so I can improve it, I guess.

It essentially scans your entire self-hosted tailscale network and finds any services
being served over the Traefik Proxy. These services change with great frequency.
This presents a problem because Headscale offers no API for configuring DNS entries directly,
thus requiring a config file change by default. This is slow and cumbersome, hence I built a tool
that deals with this problem instead.

Modus operandi
----------

Essentially it does what it says on the tin:
1. Connects to your headscale server
2. Asks it what users exist, throws out undesired users
3. Asks what nodes belong to those users and throws out undesired nodes as well
4. Using that huge list of nodes it creates an Treafik API client for each and
asks each API client what routes exist on those Traefik servers.
5. Using the route list it figures out what domains exist and filters out them
accordingly as well. 
6. The ones remaning are combined with their IP addresses obtained through Headscale
and generated into a list of domain records for Headscale to read.
7. Yay, magic.

Other motivations
----------

It also solves the problem of privately hosted services requiring an HTTPS endpoint. Since we
can't obtain the SSL certificates for ``.tailscale`` domains, as that would undermine internet
security as a whole, we need to use domains that we own. This tool allows for those domains to
be pointed towards internal tailscale IP addresses thus being available using your actual domains
while not running into the risk of an attack by exposing those services to the outside internet.

Very neat.

How to install and use
----------------

Using the nix flake file you should be able to install this directly and run it, but you can just
compile this with cargo as it's built with rust. Only system dependency you'd need are pkg-config
and openssl (for internet connectivity).

There's a provided ``.env.example`` file provided on this repository. It shows you the supported
options and what can be configued. In order to use it, change the options accordingly and rename
it to ``.env`` instead.

Once you did that, run the binary with ``.env`` file in the path of where this tool is executed
from or those variables loaded into your shell environment. The binary shouldn't emit any console 
output and just print out the final JSON file into your desired location.

License
-------

This package will be available under the terms of WTFPL, as I can't be bothered to search for a
proper license instead. I'm just not sure if any of the rust dependencies are compatible with that,
so you'll have to look into that instead.
