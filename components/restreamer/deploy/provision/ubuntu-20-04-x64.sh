#!/usr/bin/env bash

set -e

EPHYR_CLI_ARGS=${EPHYR_CLI_ARGS:-''}
EPHYR_VER=${EPHYR_VER:-edge}
if [ "$EPHYR_VER" == "latest" ]; then
  EPHYR_VER=''
else
  EPHYR_VER="-$EPHYR_VER"
fi

# Install Podman for running containers.
echo "deb https://download.opensuse.org/repositories/devel:/kubic:/libcontainers:/stable/xUbuntu_20.04/ /" \
  | tee /etc/apt/sources.list.d/devel:kubic:libcontainers:stable.list
curl -L https://download.opensuse.org/repositories/devel:/kubic:/libcontainers:/stable/xUbuntu_20.04/Release.key \
  | apt-key add -
apt-get -y update
apt-get -y install podman

WITH_FIREWALLD=${WITH_FIREWALLD:-0}
if [ "$WITH_FIREWALLD" == "1" ]; then
  # Install and setup firewalld, if required.
  apt-get -y install firewalld
  systemctl unmask firewalld.service
  systemctl enable firewalld.service
  systemctl start firewalld.service
  firewall-cmd --zone=public --permanent --add-port=80/tcp --add-port=1935/tcp
  firewall-cmd --reload
fi

# Install Ephyr re-streamer.
cat <<EOF > /etc/systemd/system/ephyr-restreamer.service
[Unit]
Description=Ephyr service for re-streaming RTMP streams
After=podman.service


[Service]
Environment=EPHYR_CONTAINER_NAME=ephyr-restreamer
Environment=EPHYR_IMAGE_NAME=docker.io/allatra/ephyr
Environment=EPHYR_IMAGE_TAG=restreamer${EPHYR_VER}

ExecStartPre=/usr/bin/mkdir -p /var/lib/\${EPHYR_CONTAINER_NAME}/
ExecStartPre=touch /var/lib/\${EPHYR_CONTAINER_NAME}/srs.conf
ExecStartPre=touch /var/lib/\${EPHYR_CONTAINER_NAME}/state.json

ExecStartPre=-/usr/bin/podman pull \${EPHYR_IMAGE_NAME}:\${EPHYR_IMAGE_TAG}
ExecStartPre=-/usr/bin/podman stop \${EPHYR_CONTAINER_NAME}
ExecStartPre=-/usr/bin/podman rm --volumes \${EPHYR_CONTAINER_NAME}
ExecStart=/usr/bin/podman run \\
  --network=host \\
  -v /var/lib/\${EPHYR_CONTAINER_NAME}/srs.conf:/usr/local/srs/conf/srs.conf \\
  -v /var/lib/\${EPHYR_CONTAINER_NAME}/state.json:/state.json \\
  --name=\${EPHYR_CONTAINER_NAME} \\
  \${EPHYR_IMAGE_NAME}:\${EPHYR_IMAGE_TAG} ${EPHYR_CLI_ARGS}

ExecStop=-/usr/bin/podman stop \${EPHYR_CONTAINER_NAME}
ExecStop=-/usr/bin/podman rm --volumes \${EPHYR_CONTAINER_NAME}

Restart=always


[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl unmask ephyr-restreamer.service
systemctl enable ephyr-restreamer.service
systemctl start ephyr-restreamer.service
