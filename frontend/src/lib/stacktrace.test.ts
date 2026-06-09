import { describe, expect, it } from 'vitest';
import {
  type Frame,
  type Stacktrace,
  frameFile,
  frameLocation,
  framesForDisplay,
  hasSourceContext,
  isInApp,
  mostRelevantFrame,
  parseEvent,
  stacktraceToText
} from './stacktrace';

function frame(partial: Partial<Frame> = {}): Frame {
  return { pre_context: [], post_context: [], ...partial };
}

describe('parseEvent — exceptions', () => {
  it('extracts the thrown (last) exception type and value', () => {
    const parsed = parseEvent({
      exception: {
        values: [
          { type: 'ValueError', value: 'inner' },
          { type: 'RuntimeError', value: 'outer' }
        ]
      }
    });
    expect(parsed.exceptionType).toBe('RuntimeError');
    expect(parsed.exceptionValue).toBe('outer');
    expect(parsed.exceptions).toHaveLength(2);
  });

  it('coerces frame fields and filters non-string context lines', () => {
    const parsed = parseEvent({
      exception: {
        values: [
          {
            type: 'Error',
            stacktrace: {
              frames: [
                {
                  function: 'main',
                  filename: 'app.ts',
                  lineno: '42',
                  colno: 7,
                  in_app: true,
                  context_line: '  boom()',
                  pre_context: ['a', 1, 'b'],
                  post_context: ['c']
                }
              ]
            }
          }
        ]
      }
    });
    const f = parsed.stacktrace?.frames[0];
    expect(f?.lineno).toBe(42);
    expect(f?.colno).toBe(7);
    expect(f?.in_app).toBe(true);
    expect(f?.pre_context).toEqual(['a', 'b']);
    expect(f?.post_context).toEqual(['c']);
  });
});

describe('parseEvent — stacktrace resolution order', () => {
  const withFrames = (fn: string) => ({ frames: [{ function: fn }] });

  it('prefers the last exception with a stacktrace', () => {
    const parsed = parseEvent({
      exception: {
        values: [{ stacktrace: withFrames('first') }, { stacktrace: withFrames('last') }]
      }
    });
    expect(parsed.stacktrace?.frames[0].function).toBe('last');
  });

  it('falls back to an earlier exception when the last has none', () => {
    const parsed = parseEvent({
      exception: { values: [{ stacktrace: withFrames('earlier') }, { type: 'NoTrace' }] }
    });
    expect(parsed.stacktrace?.frames[0].function).toBe('earlier');
  });

  it('falls back to the crashed thread', () => {
    const parsed = parseEvent({
      threads: {
        values: [
          { crashed: false, stacktrace: withFrames('healthy') },
          { crashed: true, stacktrace: withFrames('crashed') }
        ]
      }
    });
    expect(parsed.stacktrace?.frames[0].function).toBe('crashed');
  });

  it('falls back to the top-level stacktrace', () => {
    const parsed = parseEvent({ stacktrace: withFrames('toplevel') });
    expect(parsed.stacktrace?.frames[0].function).toBe('toplevel');
  });

  it('returns undefined when no frames exist anywhere', () => {
    expect(parseEvent({ exception: { values: [{ type: 'Bare' }] } }).stacktrace).toBeUndefined();
  });
});

describe('parseEvent — message and metadata', () => {
  it('reads a plain string message', () => {
    expect(parseEvent({ message: '  hello  ', level: 'error', platform: 'node' })).toMatchObject({
      message: 'hello',
      level: 'error',
      platform: 'node'
    });
  });

  it('reads a structured message object, preferring formatted', () => {
    expect(parseEvent({ message: { formatted: 'fmt', message: 'raw' } }).message).toBe('fmt');
  });

  it('falls back to logentry', () => {
    expect(parseEvent({ logentry: { message: 'from-logentry' } }).message).toBe('from-logentry');
  });

  it('returns an empty shape for non-object payloads', () => {
    const parsed = parseEvent(null);
    expect(parsed.exceptions).toEqual([]);
    expect(parsed.stacktrace).toBeUndefined();
    expect(parsed.message).toBeUndefined();
  });
});

describe('frame helpers', () => {
  it('isInApp is true only for an explicit in_app flag', () => {
    expect(isInApp(frame({ in_app: true }))).toBe(true);
    expect(isInApp(frame({ in_app: false }))).toBe(false);
    expect(isInApp(frame())).toBe(false);
  });

  it('hasSourceContext detects any context line', () => {
    expect(hasSourceContext(frame())).toBe(false);
    expect(hasSourceContext(frame({ context_line: 'x' }))).toBe(true);
    expect(hasSourceContext(frame({ pre_context: ['x'] }))).toBe(true);
    expect(hasSourceContext(frame({ post_context: ['x'] }))).toBe(true);
  });

  it('frameFile prefers filename over abs_path', () => {
    expect(frameFile(frame({ filename: 'a.ts', abs_path: '/x/a.ts' }))).toBe('a.ts');
    expect(frameFile(frame({ abs_path: '/x/a.ts' }))).toBe('/x/a.ts');
    expect(frameFile(frame())).toBeUndefined();
  });

  it('frameLocation builds the best available label', () => {
    expect(frameLocation(frame({ module: 'app', function: 'run' }))).toBe('app in run');
    expect(frameLocation(frame({ filename: 'a.ts', function: 'run' }))).toBe('a.ts in run');
    expect(frameLocation(frame({ function: 'run' }))).toBe('run');
    expect(frameLocation(frame({ filename: 'a.ts', lineno: 9 }))).toBe('a.ts:9');
    expect(frameLocation(frame({ filename: 'a.ts' }))).toBe('a.ts');
    expect(frameLocation(frame({ module: 'app' }))).toBe('app');
    expect(frameLocation(frame())).toBeUndefined();
  });
});

describe('frame ordering', () => {
  const st: Stacktrace = {
    frames: [
      frame({ function: 'oldest', in_app: false }),
      frame({ function: 'app1', in_app: true }),
      frame({ function: 'app2', in_app: true }),
      frame({ function: 'newest', in_app: false })
    ]
  };

  it('mostRelevantFrame returns the last in-app frame', () => {
    expect(mostRelevantFrame(st)?.function).toBe('app2');
  });

  it('mostRelevantFrame falls back to the last frame when none are in-app', () => {
    const noApp: Stacktrace = { frames: [frame({ function: 'a' }), frame({ function: 'b' })] };
    expect(mostRelevantFrame(noApp)?.function).toBe('b');
  });

  it('framesForDisplay reverses to crashing-frame-first order', () => {
    expect(framesForDisplay(st).map((f) => f.function)).toEqual([
      'newest',
      'app2',
      'app1',
      'oldest'
    ]);
  });
});

describe('stacktraceToText', () => {
  it('renders crashing frame first with file:line and indented source', () => {
    const st: Stacktrace = {
      frames: [
        frame({
          module: 'com.example.app',
          function: 'loadThumbnail',
          filename: 'ImageLoader.kt',
          lineno: 88
        }),
        frame({
          module: 'com.example.app',
          function: 'onBindViewHolder',
          filename: 'FeedAdapter.kt',
          lineno: 55,
          context_line: '  holder.thumbnail.setImageBitmap(item.path)'
        })
      ]
    };
    expect(stacktraceToText(st)).toBe(
      [
        '  at com.example.app in onBindViewHolder (FeedAdapter.kt:55)',
        '      holder.thumbnail.setImageBitmap(item.path)',
        '  at com.example.app in loadThumbnail (ImageLoader.kt:88)'
      ].join('\n')
    );
  });

  it('falls back to <unknown> when no location is available', () => {
    expect(stacktraceToText({ frames: [frame()] })).toBe('  at <unknown>');
  });
});
