import { parseRemoteCommand } from './remoteCommand';

const event = (body: object) => JSON.stringify({ type: 'command', ...body });

describe('parseRemoteCommand', () => {
  it('reads transport, seeking and volume', () => {
    expect(parseRemoteCommand(event({ command: 'next' }))).toEqual({ command: 'next' });
    expect(parseRemoteCommand(event({ command: 'seek', positionMs: 1500 }))).toEqual({
      command: 'seek',
      positionMs: 1500,
    });
    expect(parseRemoteCommand(event({ command: 'volume', level: 45 }))).toEqual({
      command: 'volume',
      level: 45,
    });
  });

  it('ignores other events from the player, unknown commands and missing values', () => {
    expect(parseRemoteCommand(JSON.stringify({ type: 'status', status: 'connected' }))).toBeNull();
    expect(parseRemoteCommand(event({ command: 'shuffle' }))).toBeNull();
    expect(parseRemoteCommand(event({ command: 'seek' }))).toBeNull();
    expect(parseRemoteCommand('not json')).toBeNull();
  });
});
