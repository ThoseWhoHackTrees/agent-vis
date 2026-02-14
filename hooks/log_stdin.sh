#!/bin/sh

# Read the JSON input from stdin
INPUT=$(cat)

# Determine endpoint based on hook_event_name
EVENT_NAME=$(echo "$INPUT" | grep -o '"hook_event_name"[^,}]*' | cut -d'"' -f4)

# Also log raw input for debugging
echo "$INPUT" >>"$(dirname "$0")/session_output.log"

SESSION_ID=$(echo "$INPUT" | grep -o '"session_id"[^,}]*' | cut -d'"' -f4)

case "$EVENT_NAME" in
"SessionStart")
	CWD=$(echo "$INPUT" | grep -o '"cwd"[^,}]*' | cut -d'"' -f4)
	MODEL=$(echo "$INPUT" | grep -o '"model"[^,}]*' | cut -d'"' -f4)
	PAYLOAD="{\"session_id\":\"$SESSION_ID\",\"cwd\":\"$CWD\",\"model\":\"$MODEL\"}"
	curl -s -X POST \
		-H "Content-Type: application/json" \
		-d "$PAYLOAD" \
		"http://127.0.0.1:8080/session-start" \
		>>"$(dirname "$0")/curl_debug.log" 2>&1
	;;
"PreToolUse")
	TOOL_NAME=$(echo "$INPUT" | grep -o '"tool_name"[^,}]*' | cut -d'"' -f4)
	# Try different patterns for file_path
	FILE_PATH=$(echo "$INPUT" | grep -o '"file_path"[^,}]*' | head -1 | cut -d'"' -f4)

	PAYLOAD="{\"session_id\":\"$SESSION_ID\",\"tool_name\":\"$TOOL_NAME\",\"tool_input\":{\"file_path\":\"$FILE_PATH\"}}"

	case "$TOOL_NAME" in
	"Read")
		curl -s -X POST \
			-H "Content-Type: application/json" \
			-d "$PAYLOAD" \
			"http://127.0.0.1:8080/read" \
			>>"$(dirname "$0")/curl_debug.log" 2>&1
		;;
	"Write")
		curl -s -X POST \
			-H "Content-Type: application/json" \
			-d "$PAYLOAD" \
			"http://127.0.0.1:8080/write" \
			>>"$(dirname "$0")/curl_debug.log" 2>&1
		;;
	"Edit")
		curl -s -X POST \
			-H "Content-Type: application/json" \
			-d "$PAYLOAD" \
			"http://127.0.0.1:8080/edit" \
			>>"$(dirname "$0")/curl_debug.log" 2>&1
		;;
	esac
	;;
esac
