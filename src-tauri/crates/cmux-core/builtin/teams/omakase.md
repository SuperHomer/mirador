---
description: A planner, a worker and a Codex reviewer, working the Rails doctrine's way
way: omakase
layout: columns
members:
  - role: omakase-planner
    name: planner
    lead: true
  - role: omakase-worker
    name: worker
  - role: omakase-reviewer
    name: reviewer
---
The planner owns the task: it splits it into small steps and hands them to
the worker one at a time, waiting for each. When a step is done, the
planner asks the reviewer, who reads the diff and answers the planner with
APPROVE or CHANGES. The planner decides what happens next, and tells the
person when the task is done. Nobody pushes or merges: a person does.
