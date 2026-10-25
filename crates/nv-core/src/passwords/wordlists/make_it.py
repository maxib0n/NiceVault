import re, sys
big = [w.strip() for w in open(sys.argv[1], encoding='utf-8')]
bad = set(w.strip().lower() for w in open(sys.argv[4], encoding='utf-8')) if len(sys.argv) > 4 else set()
common = set(w.strip().lower() for w in open(sys.argv[2], encoding='utf-8'))
suf = ['ava','eva','asti','isti','anno','avo','avi','avamo','avate','avano','evo','evamo','evate','evano','ivamo','ivate','ivano','erei','eremo','erete','eranno','erai','eresti','erebbe','assi','asse','assero','essi','esse','essero','issi','isse','issero','ammo','emmo','immo','aste','este','iste','arono','erono','irono','ando','endo','iamo','iate','ino','ano','ono','assimo','essimo','issimo','rei','resti','rebbe','remmo','reste','rebbero','ai','ei']
ok = []
seen = set()
for w in big:
    w = w.lower()
    if w in seen or w in bad or not re.fullmatch(r'[a-z]{4,8}', w):
        continue
    seen.add(w)
    if w[-1] not in 'aeiou':
        continue
    if w not in common and any(w.endswith(s) for s in suf):
        continue
    ok.append(w)
ok.sort(key=lambda w: (w not in common, abs(len(w) - 6), w))
pick = sorted(ok[:7776])
assert len(pick) == 7776, len(ok)
open(sys.argv[3], 'w').write('\n'.join(pick) + '\n')
print(len(ok), sum(1 for w in pick if w in common))
