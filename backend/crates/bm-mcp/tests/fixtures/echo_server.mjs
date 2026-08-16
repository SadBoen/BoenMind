// MCP stdio 测试 fixture（bm-mcp 集成测试用）：纯 Node 手写 JSON-RPC，
// 零 npm 依赖。支持 legacy initialize 握手、tools/list、tools/call；
// crash 工具模拟崩溃退出（重连测试用）。
import readline from 'node:readline';
const rl = readline.createInterface({ input: process.stdin });

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + '\n');
}

rl.on('line', (line) => {
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    return;
  }
  const respond = (result) => send({ jsonrpc: '2.0', id: msg.id, result });
  const fail = (code, message) => send({ jsonrpc: '2.0', id: msg.id, error: { code, message } });

  switch (msg.method) {
    case 'initialize':
      respond({
        protocolVersion: '2025-11-25',
        capabilities: { tools: { listChanged: true } },
        serverInfo: { name: 'echo-fixture', version: '1.0.0' },
      });
      break;
    case 'notifications/initialized':
      break; // 通知无需响应
    case 'server/discover':
      // 2.0 探测：本 fixture 是 legacy，返回 Method not found 引导回退
      fail(-32601, 'Method not found');
      break;
    case 'tools/list':
      respond({
        tools: [
          { name: 'echo', description: '回显文本', inputSchema: { type: 'object', properties: { text: { type: 'string' } } } },
          { name: 'crash', description: '模拟崩溃退出', inputSchema: { type: 'object' } },
        ],
      });
      break;
    case 'tools/call': {
      const { name, arguments: args } = msg.params || {};
      if (name === 'echo') {
        respond({ content: [{ type: 'text', text: 'echo:' + (args?.text ?? '') }] });
      } else if (name === 'crash') {
        console.error('[fixture] crash 工具被调用，进程退出');
        process.exit(1);
      } else {
        fail(-32602, 'Unknown tool: ' + name);
      }
      break;
    }
    default:
      fail(-32601, 'Method not found: ' + msg.method);
  }
});

process.stdin.on('end', () => process.exit(0));
