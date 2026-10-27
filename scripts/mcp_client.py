"""Small shared stdio MCP transport for authoring recipes; never retries writes."""
import json
import os
import select
import subprocess
import tempfile


class Mcp:
    def __init__(self, binary, workspace, url, token_file):
        command = [binary, '--workspace', str(workspace), '--headless']
        environment = os.environ.copy()
        if token_file:
            command += ['--publish-url', url, '--publish-token-file', str(token_file)]
        else:
            for key in ['ERGO_PUBLISH_URL', 'ERGO_PUBLISH_TOKEN', 'ERGO_PUBLISH_TOKEN_FILE']:
                environment.pop(key, None)
        self.log = tempfile.TemporaryFile(mode='w+t')
        self.process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self.log, text=True, env=environment)
        self.sequence = 0
        self.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                               'clientInfo': {'name': 'ergo-authoring', 'version': '1'}})

    def rpc(self, method, params):
        assert self.process.stdin is not None and self.process.stdout is not None
        self.sequence += 1
        request = {'jsonrpc': '2.0', 'id': self.sequence, 'method': method, 'params': params}
        self.process.stdin.write(json.dumps(request) + '\n')
        self.process.stdin.flush()
        if not select.select([self.process.stdout], [], [], 2100)[0]:
            raise RuntimeError('MCP timed out; no request has been automatically retried')
        line = self.process.stdout.readline()
        if not line:
            self.log.seek(0)
            raise RuntimeError('MCP exited: ' + self.log.read())
        response = json.loads(line)
        if response.get('id') != self.sequence or 'error' in response:
            raise RuntimeError(str(response.get('error', 'Unexpected MCP response ID')))
        return response['result']

    def call(self, tool_name, **arguments):
        result = self.rpc('tools/call', {'name': tool_name, 'arguments': arguments})
        if result.get('isError'):
            raise RuntimeError(json.dumps(result.get('structuredContent', result)))
        return result

    def close(self):
        assert self.process.stdin is not None
        self.process.stdin.close()
        self.process.wait(timeout=15)
        self.log.close()
