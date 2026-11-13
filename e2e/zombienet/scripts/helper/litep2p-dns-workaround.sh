#!/usr/bin/env bash

# Copyright (C) Parity Technologies (UK) Ltd.
# This file is part of Individuality.
# SPDX-License-Identifier: Apache-2.0
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Sourced by the spawn scripts after they put .tools/bin on PATH. Expects HERE to be set.
#
# The collator's embedded relay node uses litep2p by default. litep2p cannot read a resolv.conf
# that has a scoped IPv6 nameserver, e.g. `nameserver fe80::1%en0`, which macOS writes on some
# networks. The People collator then exits at start with "Cannot read system DNS config".
#
# On such a host, this puts a wrapper for polkadot-omni-node first on PATH. The wrapper gives the
# embedded relay node `--network-backend libp2p`. The arguments after `--` configure that node, so
# the wrapper appends the flag only when `--` is present.

if grep -Eq '^[[:space:]]*nameserver[[:space:]]+[^[:space:]]*%' /etc/resolv.conf 2>/dev/null; then
	wrap_dir="$HERE/.tools/wrap"
	real_omni_node="$(command -v polkadot-omni-node)"
	mkdir -p "$wrap_dir"
	cat >"$wrap_dir/polkadot-omni-node" <<WRAPPER
#!/usr/bin/env bash
for arg in "\$@"; do
	if [ "\$arg" = "--" ]; then exec "$real_omni_node" "\$@" --network-backend libp2p; fi
done
exec "$real_omni_node" "\$@"
WRAPPER
	chmod +x "$wrap_dir/polkadot-omni-node"
	export PATH="$wrap_dir:$PATH"
	echo "scoped IPv6 nameserver in /etc/resolv.conf: the collators' relay nodes use libp2p"
fi
