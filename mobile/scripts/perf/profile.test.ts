import { descriptorOf, methodDescriptor, toBaselineProfile } from './profile.ts';

describe('descriptorOf', () => {
  it('writes primitives, classes and arrays', () => {
    expect(descriptorOf('void')).toBe('V');
    expect(descriptorOf('int')).toBe('I');
    expect(descriptorOf('java.lang.String')).toBe('Ljava/lang/String;');
    expect(descriptorOf('int[][]')).toBe('[[I');
    expect(descriptorOf('android.text.InputFilter[]')).toBe('[Landroid/text/InputFilter;');
  });
});

describe('methodDescriptor', () => {
  it('writes a method with its parameters and return type', () => {
    expect(methodDescriptor('void a.B.c(int, a.B)')).toBe('La/B;->c(ILa/B;)V');
  });

  it('writes constructors and nested classes', () => {
    expect(methodDescriptor('void a.B$1.<init>(a.B)')).toBe('La/B$1;-><init>(La/B;)V');
  });

  it('writes a method with no parameters', () => {
    expect(methodDescriptor('boolean a.B.c()')).toBe('La/B;->c()Z');
  });

  it('refuses a line that is not a method', () => {
    expect(methodDescriptor('a.B')).toBeNull();
  });
});

const DUMP = `=== Dex files  ===
base.apk [checksum=7f71f9cf]
=== profile ===
ProfileInfo [015]

base.apk [index=0] [checksum=7f71f9cf] [num_type_ids=9868] [num_method_ids=65518]
\thot methods: 
\t\tvoid a.B.hot()[], 
\t\tvoid a.B.both(int)[], 
\tstartup methods: 
\t\tvoid a.B.both(int)
\t\tvoid a.B.<init>()
\tpost startup methods: 
\t\tvoid a.B.later()
\tclasses: 
\t\ta.B
\t\ta.C$1
base.apk!classes2.dex [index=1] [checksum=c886572b] [num_type_ids=9304]
\tstartup methods: 
\t\tjava.lang.String d.E.name(java.lang.String[])
\tclasses: 
\t\td.E
=== reference profile ===
ProfileInfo [015]
-empty-
`;

describe('toBaselineProfile', () => {
  const lines = toBaselineProfile(DUMP);

  it('keeps every class that was loaded', () => {
    expect(lines).toContain('La/B;');
    expect(lines).toContain('La/C$1;');
    expect(lines).toContain('Ld/E;');
  });

  it('gives a method the flags of every section it appears in', () => {
    expect(lines).toContain('HSLa/B;->both(I)V');
    expect(lines).toContain('HLa/B;->hot()V');
    expect(lines).toContain('SLa/B;-><init>()V');
    expect(lines).toContain('PLa/B;->later()V');
  });

  it('reads every dex file in the profile', () => {
    expect(lines).toContain('SLd/E;->name([Ljava/lang/String;)Ljava/lang/String;');
  });

  it('leaves out the dump around the profile, and is sorted', () => {
    expect(lines.filter((line) => line.includes('ProfileInfo'))).toEqual([]);
    expect(lines.filter((line) => line.includes('base.apk'))).toEqual([]);
    expect([...lines].sort()).toEqual(lines);
  });
});
