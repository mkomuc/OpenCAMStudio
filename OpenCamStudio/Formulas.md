
### Roughing Passes

| Operation | Section         | Parameter                 | Formula                                                                   |
| --------- | --------------- | ------------------------- | ------------------------------------------------------------------------- |
| Profile   | Roughing Passes | maximum Stepover          | max((tool_diameter - (2 * tool_cornerRadius)) * 0.95; tool_flutes * 0.20) |
| Pocket    | Passes          | Optimal Load              | tool_diameter * 0.4                                                       |
|           | Multiple Depths | Maximum Roughing Stepdown | min(tool_fluteLength * 0.75; tool_diameter * 2.5)                         |
