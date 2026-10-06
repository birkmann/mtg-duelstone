# The public engine behind the website's "Play in your browser": Forge + its AI, serving the browser
# client, in --public mode (vs-AI games only, capped, nothing written for visitors).
#
# Built from a staged bundle, not from source: packaging/server/build.sh runs stage-bundle.sh (jlink'd
# JRE, Forge data, engine jar, web client) and uses that directory as the build context.
#
# Card images are not baked in (9 GB, and Wizards' art stays out of published artifacts): mount a
# filled offline/img at /opt/mtg-duelstone/offline/img. Without it cards show as text frames.
FROM debian:bookworm-slim

RUN useradd --system --create-home --home-dir /data duelstone
COPY --chown=root:root . /opt/mtg-duelstone

# Forge keeps preferences and its card cache under $HOME/.forge
ENV HOME=/data \
    MTG_DUELSTONE_PUBLIC=1 \
    MTG_DUELSTONE_MAX_MATCHES=6
USER duelstone
VOLUME ["/data"]
EXPOSE 8080
# Heap follows the container's memory limit: every live game holds a Forge game and its AI threads.
ENTRYPOINT ["/opt/mtg-duelstone/jre/bin/java", "-Djava.awt.headless=true", "-XX:MaxRAMPercentage=80", \
    "-jar", "/opt/mtg-duelstone/engine/forge-ws-server.jar", \
    "--port", "8080", \
    "--assets", "/opt/mtg-duelstone/forge", \
    "--offline", "/opt/mtg-duelstone/offline", \
    "--static", "/opt/mtg-duelstone/web"]
