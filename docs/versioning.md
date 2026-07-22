# Версионирование Uprava

Статус: `active`

Uprava использует Semantic Versioning для implementation and release baselines:

```text
MAJOR.MINOR.PATCH
```

До `1.0.0` поле `MINOR` обозначает цельный продуктовый baseline, а `PATCH`
обозначает завершенные feature or fix slices поверх этого baseline. Public
compatibility contract до `1.0.0` еще может меняться, но каждый version bump
должен делать состояние репозитория понятнее.

## Правила именования

- Используем `0.x.y` для pre-production development releases.
- Используем `0.1.0` для первой shipped working baseline.
- Увеличиваем `PATCH` для завершенных implementation slices, которые не
  переопределяют продуктовый baseline.
- Увеличиваем `MINOR`, когда product shape or architecture baseline меняется
  настолько, что downstream docs and runbooks требуют нового baseline.
- Резервируем `1.0.0` для первого production-ready compatibility and security
  contract.

## Product Cuts And Release Versions

`V01` - это имя продуктового среза, а не SemVer version. Оно описывает первый
цельный product scope, который был shipped как `0.1.0`.

Release versions описывают состояние репозитория и реализации. Current planning
documents должны ссылаться на текущий release baseline, а не на `V01`, когда
они обсуждают features, доставленные после `0.1.0`.

Current baseline: `0.2.25`.

## Release Candidates 0.2.0

- Каждый candidate build с source changes использует уникальную SemVer
  pre-release version `0.2.0-rc.N`; `N` растёт монотонно, и candidate version
  никогда не пересобирается с другим содержимым.
- У каждого candidate также есть immutable Git-SHA-based release id. Core, Web
  and Node artifacts этого candidate имеют общие version, Git SHA and release
  id.
- Release candidates фиксируются во временном RC checklist и build manifest, а
  не как current shipped baseline в [`releases.md`](releases.md).
- Final version `0.2.0` назначается только после прохождения final RC полного
  clean-state release gate. Final build обязан повторно пройти тот же gate. При
  failure он отбрасывается, работа возвращается к следующему `0.2.0-rc.N`, fix
  и проверка повторяются; нельзя публиковать два разных artifacts `0.2.0`.
- `0.2.0` — coordinated breaking protocol-v2 release. Compatibility с API,
  schemas или state 0.1.x и in-place state migration не являются release
  requirements.

## Правила обновления

Когда закрывается пункт feature queue или другой большой этап работы:

1. Обновить [`releases.md`](releases.md) с version and completed slice.
2. Обновить package metadata, если меняется текущая implementation version.
3. Обновить temporary plans, если они ссылаются на устаревший scope.
4. Перенести долговечные продуктовые, архитектурные и процессные решения в
   канонические русские документы в `docs/`.

Temporary plans могут сохранять historical references to `V01`, но не должны
использовать `V01` как shorthand для текущей реализации после shipped
post-`0.1.0` slices.
