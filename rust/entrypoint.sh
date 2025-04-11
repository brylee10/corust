#!/bin/bash

set -eu

timeout=${CORUST_TIMEOUT_SEC:-15}

# Prints "Killed" if the timeout is reached
timeout --signal KILL "${timeout}s" "$@" 