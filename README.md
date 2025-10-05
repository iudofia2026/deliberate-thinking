# Deliberate Thinking MCP Server

A structured thinking [Model Context Protocol](https://modelcontextprotocol.io/docs/getting-started/intro)
tool for AI assistants that breaks down complex problems into sequential,
revisable thoughts.

## Notes

* This is based on the [Sequential Thinking](https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking)
MCP
* The project goal is merely to be useful to _me_ and _my_ work. It's
  easy to fork for your work.
* This is written in Rust merely for low-latency start-up times
  and for fun.


## Quick Start

### Install

```bash
# Clone the repository
git clone https://github.com/kljensen/deliberate-thinking.git
cd deliberate-thinking
cargo build --release
```

### Adding deliberate thinking to your AI assistant

You can find instructions for your assistants at these links:
- [Claude Code MCP instructions](https://docs.claude.com/en/docs/claude-code/mcp)
- [OpenAI Codex MCP instructions](https://github.com/openai/codex/blob/main/docs/advanced.md#model-context-protocol-mcp)
- [GitHub Copilot MCP instructions](https://docs.github.com/en/copilot/how-tos/provide-context/use-mcp/extend-copilot-chat-with-mcp)

For Claude Code, I often have a `.mcp.json` file in my working directory with the following content.

```json
{
  "mcpServers": {
    "deliberate-thinking": {
      "command": "/your/path/to/deliberate-thinking-server",
      "args": []
    }
  }
}
```

## License

The [Unlicense](https://unlicense.org/).
## Collaborative Product Team

Deliberate Thinking now coordinates a small agile team so the tool can drive end-to-end product work without leaving the MCP flow.

- **Project manager (`ProjectManager`)** keeps the squad aligned, captures discussion points, maintains the backlog, and delivers bullet summaries with priorities, sprint intent, and consensus.
- **Pragmatic programmer (`PragmaticProgrammer`)** focuses on feasibility and implementation detail, favouring clean, minimal code changes.
- **Product visionary (`ProductVisionary`)** pushes on differentiation and revenue potential when shaping user stories and product bets.

### Request Additions

`DeliberateThinkingRequest` accepts several optional fields so each call can document team collaboration:

- `role`: which teammate is talking for this update.
- `discussionPoints`: structured notes `{ "role": "<role>", "detail": "<note>" }` captured for the PM synopsis.
- `backlogStories` / `removeStoryIds`: create, update, or retire user stories with priority and status.
- `sprintPlan`: agile sprint plan with participants, commitments, and risks.
- `consensusUpdate`: ready-for-code-change flag plus blockers and notes.
- `requiresUserInput`: toggle when the team needs guidance before committing changes.

### Response Shape

`DeliberateThinkingResponse` now returns an additional `pmReport` payload. It contains:

- `bullets`: immediately usable, structured PM summary lines (progress, risks, sprint focus).
- `pmSummary`: the latest narrative from the project manager.
- `newDiscussionPoints`, `backlogSnapshot`, `activeSprint`, and `consensus`: machine-readable state the assistant can reason over between calls.
- `waitingOnUser`: whether the squad is paused for your decision.

#### Example

```json
{
  "thought": "Sprint review and planning",
  "role": "ProjectManager",
  "discussionPoints": [
    { "role": "<role>", "detail": "<note>" },
    { "role": "<role>", "detail": "<note>" }
  ],
  "backlogStories": [
    {
      "id": "STORY-101",
      "title": "Export analytics dashboard",
      "priority": "High",
      "status": "Todo"
    }
  ],
  "sprintPlan": {
    "sprint_name": "Sprint 7",
    "goal": "Ship analytics export MVP",
    "duration_days": 7,
    "participants": [
      {
        "role": "ProjectManager",
        "reasoning": "Coordinate scope and stakeholder communication"
      },
      {
        "role": "PragmaticProgrammer",
        "reasoning": "Implement CSV export and guardrails"
      }
    ],
    "committed_story_ids": ["STORY-101"]
  },
  "consensusUpdate": {
    "ready_for_code_changes": false,
    "blockers": ["Need confirmation on pricing tier"]
  },
  "requiresUserInput": true
}
```

The PM report returned for every call stays in bullet form so you can skim priorities, backlog movement, and sprint actions at a glance.
