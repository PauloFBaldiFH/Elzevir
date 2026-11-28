<!-- Gerado por `cargo xtask codegen` a partir de data/koha-<série>/sysprefs.json e tools/sysprefs-triage.json. Não edite à mão. -->

# Para administradores do Koha: cada syspref ao lado do seu campo

As 915 preferências do sistema do Koha e o lugar de cada uma no Elzevir. As que têm campo no `elzevir.toml` são trazidas por `elzevir import-koha-prefs` ([preferencias-do-koha.md](preferencias-do-koha.md)); as outras, quando o valor da biblioteca difere do padrão do Koha, vão para `[koha_legacy]`, para revisão.

## (ocultas)

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AccessControlAllowOrigin` | (vazio) | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `ActionLogsTraceDepth` | `0` | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `ChildNeedsGuarantor` | `0` | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `ElasticsearchIndexStatus_authorities` | `0` | Comportamento fixo: Busca sempre Tantivy. |
| `ElasticsearchIndexStatus_biblios` | `0` | Comportamento fixo: Busca sempre Tantivy. |
| `OAI-PMH:AutoUpdateSets` | `0` | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `OAI-PMH:AutoUpdateSetsEmbedItemData` | `0` | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `OAI-PMH:ConfFile` | (vazio) | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `OAI-PMH:DeletedRecord` | `persistent` | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `OAI-PMH:HarvestEmailReport` | (vazio) | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `OAI-PMH:MaxCount` | `50` | **`[oai] max_count`**. Ver docs/specs/opac/oai-pmh.md, item 2. |
| `OAI-PMH:archiveID` | `KOHA-OAI-TEST` | **`[oai] repository_identifier`**. Ver docs/specs/opac/oai-pmh.md, item 2. |
| `OPACdidyoumean` | (vazio) | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `RESTdefaultPageSize` | `20` | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `UsageStatsID` | (vazio) | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `UsageStatsLastUpdateTime` | (vazio) | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |
| `UsageStatsPublicID` | (vazio) | Plataforma: Preferência oculta no Koha (estado interno, OAI-PMH, estatísticas de uso). |

## Accounting

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AccountAutoReconcile` | `0` | A decidir: Decidir na spec de multas e pagamentos (Fase 3). |
| `AutoCreditNumber` | (vazio) | A decidir: Decidir na spec de multas e pagamentos (Fase 3). |
| `EnablePointOfSale` | `0` | A decidir: Decidir na spec de multas e pagamentos (Fase 3). |
| `FinePaymentAutoPopup` | `0` | A decidir: Decidir na spec de multas e pagamentos (Fase 3). |
| `RequireCashRegister` | `0` | A decidir: Decidir na spec de multas e pagamentos (Fase 3). |
| `RequirePaymentType` | `0` | A decidir: Decidir na spec de multas e pagamentos (Fase 3). |
| `RoundFinesAtPayment` | `0` | A decidir: Decidir na spec de multas e pagamentos (Fase 3). |
| `UseCashRegisters` | `0` | A decidir: Decidir na spec de multas e pagamentos (Fase 3). |

## Acquisitions

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `1PageOrderPDFText` | `Order number must appear on all related correspondence, ship…` | Fora da v1: Aquisições não entram na v1. |
| `AcqCreateItem` | `ordering` | Fora da v1: Aquisições não entram na v1. |
| `AcqEnableFiles` | `0` | Fora da v1: Aquisições não entram na v1. |
| `AcqItemSetSubfieldsWhenReceiptIsCancelled` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `AcqItemSetSubfieldsWhenReceived` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `AcqViewBaskets` | `user` | Fora da v1: Aquisições não entram na v1. |
| `AcqWarnOnDuplicateInvoice` | `0` | Fora da v1: Aquisições não entram na v1. |
| `AcquisitionsDefaultEmailAddress` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `AcquisitionsDefaultReplyTo` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `BasketConfirmations` | `1` | Fora da v1: Aquisições não entram na v1. |
| `CalculateFundValuesIncludingTax` | `1` | Fora da v1: Aquisições não entram na v1. |
| `CancelOrdersInClosedBaskets` | `0` | Fora da v1: Aquisições não entram na v1. |
| `ClaimsBccCopy` | `0` | Fora da v1: Aquisições não entram na v1. |
| `CurrencyFormat` | `US` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([instituicao] formato_moeda), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `EDIFACT` | `0` | Fora da v1: Aquisições não entram na v1. |
| `EdifactInvoiceImport` | `automatic` | Fora da v1: Aquisições não entram na v1. |
| `EdifactLSL` | `ccode` | Fora da v1: Aquisições não entram na v1. |
| `EdifactLSQ` | `location` | Fora da v1: Aquisições não entram na v1. |
| `EmailAddressForSuggestions` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `EmailPurchaseSuggestions` | `0` | Fora da v1: Aquisições não entram na v1. |
| `MarcFieldsToOrder` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `MarcItemFieldsToOrder` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `MarcOrderingAutomation` | `0` | Fora da v1: Aquisições não entram na v1. |
| `OrderPdfFormat` | `pdfformat::layout3pages` | Fora da v1: Aquisições não entram na v1. |
| `OrderPriceRounding` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `PurgeSuggestionsOlderThan` | (vazio) | Fora da v1: Aquisições não entram na v1. |
| `TaxRates` | `0` | Fora da v1: Aquisições não entram na v1. |
| `UniqueItemFields` | `barcode` | Fora da v1: Aquisições não entram na v1. |
| `UseACQFrameworkForBiblioRecords` | `0` | Fora da v1: Aquisições não entram na v1. |

## Administration

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AddressForFailedOverdueNotices` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `AllowPKIAuth` | `None` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `CSVDelimiter` | `,` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `DebugLevel` | `2` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `DefaultToLoggedInLibraryCircRules` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `DefaultToLoggedInLibraryNoticesSlips` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `DefaultToLoggedInLibraryOverdueTriggers` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `ElasticsearchMARCFormat` | `base64ISO2709` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `EmailOverduesNoEmail` | `1` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `ForceLibrarySelection` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `GoogleOAuth2ClientID` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `GoogleOAuth2ClientSecret` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). Senha ou chave: não é copiada. |
| `GoogleOpenIDConnect` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `GoogleOpenIDConnectAutoRegister` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `GoogleOpenIDConnectDefaultBranch` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `GoogleOpenIDConnectDefaultCategory` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `GoogleOpenIDConnectDomain` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `IndependentBranches` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `IndependentBranchesPatronModifications` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `IndependentBranchesTransfers` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `JobsNotificationMethod` | `STOMP` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `KohaAdminEmailAddress` | `root@localhost` | **`[oai] admin_email`**. O endereço que o Identify do OAI-PMH informa; docs/specs/opac/oai-pmh.md, item 2. |
| `ReplytoDefault` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `ReturnpathDefault` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `SearchEngine` | `Zebra` | Comportamento fixo: Busca sempre Tantivy. |
| `SendAllEmailsTo` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `SessionRestrictionByIP` | `1` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `SessionStorage` | `mysql` | Comportamento fixo: Sessões do Elzevir são próprias (doc 05). |
| `StaffLoginLibraryBasedOnIP` | `1` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `StaffLoginRestrictLibraryByIP` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `UsageStats` | `2` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `UsageStatsCountry` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `UsageStatsGeolocation` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `UsageStatsLibrariesInfo` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `UsageStatsLibraryName` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `UsageStatsLibraryType` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `UsageStatsLibraryUrl` | (vazio) | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `casAuthentication` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `casLogout` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `casServerUrl` | `https://localhost:8443/cas` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `casServerVersion` | `2` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `noItemTypeImages` | `0` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |
| `timeout` | `1d` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([equipe] sessao_ociosa), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `virtualshelves` | `1` | Plataforma: Coberto pela plataforma do Elzevir (painel, elzevir.toml de servidor, sessões, SMTP). |

## Authorities

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AdditionalFieldsInZ3950ResultAuthSearch` | (vazio) | A decidir: Decidir na spec de autoridades. |
| `AuthDisplayHierarchy` | `0` | A decidir: Decidir na spec de autoridades. |
| `AuthorityControlledIndicators` | `# PERSO_NAME  100 600 696 700 796 800 896 marc21, 100, ind1:…` | A decidir: Decidir na spec de autoridades. |
| `AuthorityMergeLimit` | `50` | A decidir: Decidir na spec de autoridades. |
| `AuthorityMergeMode` | `loose` | A decidir: Decidir na spec de autoridades. |
| `AutoCreateAuthorities` | `0` | A decidir: Decidir na spec de autoridades. |
| `AutoLinkBiblios` | `0` | A decidir: Decidir na spec de autoridades. |
| `CatalogModuleRelink` | `0` | A decidir: Decidir na spec de autoridades. |
| `ConsiderHeadingUse` | `0` | A decidir: Decidir na spec de autoridades. |
| `DefaultAuthorityTab` | `0` | A decidir: Decidir na spec de autoridades. |
| `GenerateAuthorityField667` | `Machine generated authority record` | A decidir: Decidir na spec de autoridades. |
| `GenerateAuthorityField670` | `Work cat.` | A decidir: Decidir na spec de autoridades. |
| `LanguageToUseOnMerge` | (vazio) | A decidir: Decidir na spec de autoridades. |
| `LinkerConsiderDiacritics` | `0` | A decidir: Decidir na spec de autoridades. |
| `LinkerConsiderThesaurus` | `0` | A decidir: Decidir na spec de autoridades. |
| `LinkerKeepStale` | `0` | A decidir: Decidir na spec de autoridades. |
| `LinkerModule` | `Default` | A decidir: Decidir na spec de autoridades. |
| `LinkerOptions` | (vazio) | A decidir: Decidir na spec de autoridades. |
| `LinkerRelink` | `1` | A decidir: Decidir na spec de autoridades. |
| `MARCAuthorityControlField008` | `\|\| aca\|\|aabn           \| a\|a     d` | A decidir: Decidir na spec de autoridades. |
| `RequireChoosingExistingAuthority` | `0` | A decidir: Decidir na spec de autoridades. |
| `UNIMARCAuthorityField100` | `afrey50      ba0` | A decidir: Decidir na spec de autoridades. |
| `UseAuthoritiesForTracings` | `1` | A decidir: Decidir na spec de autoridades. |

## Cataloging

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AcquisitionDetails` | `1` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `AdditionalFieldsInZ3950ResultSearch` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `AggressiveMatchOnISBN` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `AggressiveMatchOnISSN` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `AlternateHoldingsField` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `AlternateHoldingsSeparator` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `AlwaysShowHoldingsTableFilters` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `ApplyFrameworkDefaults` | `new` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `AuthoritySeparator` | `--` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `BibtexExportAdditionalFields` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `CatalogConcerns` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `CatalogerEmails` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `ComponentSortField` | `title` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `ComponentSortOrder` | `asc` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `ContentWarningField` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `CreateAVFromCataloguing` | `1` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `DefaultClassificationSource` | `ddc` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `DefaultCountryField008` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `DefaultLanguageField008` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `DefaultSaveRecordFileID` | `biblionumber` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `EasyAnalyticalRecords` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `EnableAdvancedCatalogingEditor` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `EnableItemGroups` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `ISBD` | `#100\|\|{ 100a }{ 100b }{ 100c }{ 100d }{ 110a }{ 110b }{ 110c…` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `IntranetBiblioDefaultView` | `normal` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `LabelMARCView` | `standard` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MARCOrgCode` | `OSt` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MARCOverlayRules` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MarcFieldDocURL` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MarcFieldForCreatorId` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MarcFieldForCreatorName` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MarcFieldForModifierId` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MarcFieldForModifierName` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MaxComponentRecords` | `300` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `MergeReportFields` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `NewItemsDefaultLocation` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `NotesToHide` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `OpacSuppression` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `OpacSuppressionByIPRange` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `OpacSuppressionRedirect` | `1` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `PrefillItem` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `RisExportAdditionalFields` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `SeparateHoldings` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `SeparateHoldingsBranch` | `homebranch` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `ShowComponentRecords` | `nowhere` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `SpineLabelAutoPrint` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `SpineLabelFormat` | `<itemcallnumber><copynumber>` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `StripWhitespaceChars` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `SubfieldsToAllowForRestrictedBatchmod` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `SubfieldsToAllowForRestrictedEditing` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `SubfieldsToUseWhenPrefill` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `UNIMARCField100Language` | `fre` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `URLLinkText` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `UseControlNumber` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `UseOCLCEncodingLevels` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `advancedMARCeditor` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `autoBarcode` | `OFF` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `autoControlNumber` | `OFF` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `hide_marc` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `item-level_itypes` | `1` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `itemcallnumber` | (vazio) | A decidir: Decidir na spec de catalogação (Fase 1). |
| `z3950AuthorAuthFields` | `701,702,700` | A decidir: Decidir na spec de catalogação (Fase 1). |
| `z3950NormalizeAuthor` | `0` | A decidir: Decidir na spec de catalogação (Fase 1). |

## Circulation

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AgeRestrictionMarker` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AgeRestrictionOverride` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllFinesNeedOverride` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowAllMessageDeletion` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowCheckoutNotes` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowFineOverride` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowFineOverrideRenewing` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowHoldCheckoutOverride` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowHoldDateInFuture` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowHoldItemTypeSelection` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowHoldPolicyOverride` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowHoldsOnDamagedItems` | `1` | Comportamento fixo: Padrão do Koha (ligado): exemplar danificado pode ser reservado e atende reservas (lost-items.md, item 7). |
| `AllowHoldsOnPatronsPossessions` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowItemsOnHoldCheckoutSCO` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowItemsOnHoldCheckoutSIP` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowItemsOnLoanCheckoutSIP` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowMultipleIssuesOnABiblio` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowNotForLoanOverride` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowRenewalIfOtherItemsAvailable` | `0` | Comportamento fixo: Fixo desligado, como o padrão do Koha: qualquer reserva que o exemplar atenderia bloqueia a renovação (docs/specs/circulacao/renovacao.md). |
| `AllowRenewalLimitOverride` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowRenewalOnHoldOverride` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowReturnToBranch` | `anywhere` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowSetAutomaticRenewal` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AllowTooManyOverride` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AlwaysLoadCheckoutsTable` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ArticleRequests` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ArticleRequestsLinkControl` | `calc` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ArticleRequestsMandatoryFields` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ArticleRequestsMandatoryFieldsItemOnly` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ArticleRequestsMandatoryFieldsRecordOnly` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ArticleRequestsOpacHostRedirection` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ArticleRequestsSupportedFormats` | `PHOTOCOPY` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutoClaimReturnStatusOnCheckin` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutoClaimReturnStatusOnCheckout` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutoRemoveOverduesRestrictions` | `no` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutoResumeSuspendedHolds` | `1` | Comportamento fixo: 1: a reserva suspensa até um dia volta à fila nesse dia (docs/specs/circulacao/reservas.md, item 19). |
| `AutoReturnCheckedOutItems` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutoSelfCheckAllowed` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutoSelfCheckID` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutoSelfCheckPass` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). Senha ou chave: não é copiada. |
| `AutoSwitchPatron` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutomaticCheckinAutoFill` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutomaticConfirmTransfer` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutomaticItemReturn` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `AutomaticRenewalPeriodBase` | `date_due` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `BatchCheckouts` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `BatchCheckoutsValidCategories` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `BlockReturnOfLostItems` | `0` | Comportamento fixo: Padrão do Koha (0): a devolução acha o exemplar perdido (lost-items.md, item 9). |
| `BlockReturnOfWithdrawnItems` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `BranchTransferLimitsType` | `ccode` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `BundleLostValue` | `5` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `BundleNotLoanValue` | `3` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CalculateFinesOnBackdate` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CalculateFinesOnReturn` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CanMarkHoldsToPullAsLost` | `do_not_allow` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ChargeFinesOnClosedDays` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CircAutoPrintQuickSlip` | `qslip` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CircConfirmItemParts` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CircControl` | `ItemHomeLibrary` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CircControlReturnsBranch` | `ItemHomeLibrary` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CircSidebar` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ClaimReturnedChargeFee` | `ask` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ClaimReturnedLostValue` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ClaimReturnedWarningThreshold` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CleanUpDatabaseReturnClaims` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ConfirmFutureHolds` | `0` | Comportamento fixo: 0: sem reservas futuras (docs/specs/circulacao/reservas.md, §6). |
| `ConsiderLibraryHoursInCirculation` | `close` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ConsiderOnSiteCheckoutsAsNormalCheckouts` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CumulativeRestrictionPeriods` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `CurbsidePickup` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `DefaultHoldExpirationdate` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `DefaultHoldExpirationdatePeriod` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `DefaultHoldExpirationdateUnitOfTime` | `days` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `DefaultHoldPickupLocation` | `loggedinlibrary` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `DefaultLongOverdueChargeValue` | (vazio) | **`[circulation.long_overdue] charge`**. Ligado quando é igual a DefaultLongOverdueLostValue; docs/specs/circulacao/conta.md, item 22. |
| `DefaultLongOverdueDays` | (vazio) | **`[circulation.long_overdue] days`**. Ver docs/specs/circulacao/conta.md, item 20. |
| `DefaultLongOverdueLostValue` | (vazio) | **`[circulation.long_overdue] lost_status`**. Ver docs/specs/circulacao/conta.md, item 20. |
| `DefaultLongOverduePatronCategories` | (vazio) | Comportamento fixo: Sem filtro por categoria: o atraso longo vale para todos (docs/specs/circulacao/conta.md, §5). |
| `DefaultLongOverdueSkipLostStatuses` | (vazio) | Comportamento fixo: Só o exemplar que já tem a situação de perda configurada é ignorado (docs/specs/circulacao/conta.md, §5). |
| `DefaultLongOverdueSkipPatronCategories` | (vazio) | Comportamento fixo: Sem filtro por categoria: o atraso longo vale para todos (docs/specs/circulacao/conta.md, §5). |
| `DisplayAddHoldGroups` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `DisplayClearScreenButton` | `no` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `DisplayMultiItemHolds` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `DisplayMultiPlaceHold` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `EmailPatronWhenHoldIsPlaced` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `EnableBooking` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `EnableItemGroupHolds` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ExcludeHolidaysFromMaxPickUpDelay` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ExpireReservesAutoFill` | `0` | Comportamento fixo: Desligada: a próxima reserva é achada na devolução seguinte (docs/specs/circulacao/reservas.md, item 18). |
| `ExpireReservesAutoFillEmail` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ExpireReservesMaxPickUpDelay` | `0` | **`[circulation.holds] expire_waiting`**. Ver docs/specs/circulacao/reservas.md, item 18. |
| `ExpireReservesMaxPickUpDelayCharge` | `0` | Comportamento fixo: 0: sem cobrança ao cancelar a reserva vencida (docs/specs/circulacao/reservas.md, §6). |
| `ExpireReservesOnHolidays` | `1` | Comportamento fixo: Ligada: a reserva vencida é cancelada mesmo em dia fechado (docs/specs/circulacao/reservas.md, item 18). |
| `ExportCircHistory` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ExportRemoveFields` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `FilterBeforeOverdueReport` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `FineNoRenewals` | `100` | Comportamento fixo: Fixo no padrão do Koha, 100 na moeda: saldo acima bloqueia a renovação (docs/specs/circulacao/renovacao.md). |
| `FineNoRenewalsBlockAutoRenew` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `FineNoRenewalsBlockSelfCheckRenew` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `FineNotifyAtCheckin` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `FinesIncludeGracePeriod` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `FutureHoldsBlockRenewals` | `0` | Comportamento fixo: Fixo desligado, como o padrão do Koha; o Elzevir ainda não tem reserva com data futura (docs/specs/circulacao/renovacao.md). |
| `HidePersonalPatronDetailOnCirculation` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldCancellationRequestSIP` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldFeeMode` | `not_always` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldRatioDefault` | `3` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldsAutoFill` | `0` | Comportamento fixo: Desligada: a equipe confirma na devolução (docs/specs/circulacao/reservas.md, item 14). |
| `HoldsAutoFillPrintSlip` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldsNeedProcessingSIP` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldsQueueParallelLoopsCount` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldsQueuePrioritizeBranch` | `homebranch` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldsQueueSkipClosed` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldsSplitQueue` | `nothing` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldsSplitQueueNumbering` | `actual` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HoldsToPullStartDate` | `2` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HomeOrHoldingBranch` | `holdingbranch` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `HouseboundModule` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `IssueLostItem` | `alert` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `IssuingInProcess` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ItemsDeniedRenewal` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `LoadCheckoutsTableDelay` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `LocalHoldsPriority` | `None` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `LocalHoldsPriorityItemControl` | `holdingbranch` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `LocalHoldsPriorityPatronControl` | `PickupLibrary` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `LostChargesControl` | `ItemHomeLibrary` | Comportamento fixo: Padrão do Koha (ItemHomeLibrary com holdingbranch): a cobrança da perda é da unidade onde o exemplar está (lost-items.md, item 4). |
| `ManualRenewalPeriodBase` | `date_due` | **`[circulation] renewal_base`**. Ver docs/specs/circulacao/renovacao.md, item 5. |
| `MarkLostItemsAsReturned` | `batchmod,moredetail,cronjob,additem,pendingreserves,onpaymen…` | Comportamento fixo: Padrão do Koha para a rotina: a perda cobrada termina o empréstimo; mark_returned no elzevir.toml faz o mesmo sem cobrança (conta.md, item 22). |
| `NoIssuesChargeGuarantees` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `NoIssuesChargeGuarantorsWithGuarantees` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `NoRefundOnLostFinesPaidAge` | (vazio) | Comportamento fixo: Vazio no Koha: sem limite de idade para o reembolso (lost-items.md, item 16). |
| `NoRefundOnLostReturnedItemsAge` | (vazio) | Comportamento fixo: Vazio no Koha: sem limite de idade para o reembolso (lost-items.md, item 16). |
| `NoRenewalBeforePrecision` | `exact_time` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `NoticeBcc` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OPACAllowHoldDateInFuture` | `0` | Comportamento fixo: Fixo desligado: o leitor não reserva com data futura pelo OPAC (docs/specs/opac/conta-do-leitor.md). |
| `OPACAllowUserToChangeBranch` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OPACAllowUserToChooseBranch` | `1` | Comportamento fixo: Fixo ligado: o leitor escolhe a unidade de retirada, com a própria já escolhida (docs/specs/opac/conta-do-leitor.md). |
| `OPACHoldsIfAvailableAtPickup` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OPACHoldsIfAvailableAtPickupExceptions` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OPACMandatoryHoldDates` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OnSiteCheckoutAutoCheck` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OnSiteCheckouts` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OnSiteCheckoutsForce` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OverdueNoticeCalendar` | `0` | Comportamento fixo: Desligado, o padrão do Koha: o atraso do aviso conta dias de calendário. O domínio já conta os dias fechados se for ligado (docs/specs/circulacao/avisos.md, item 9). |
| `OverdueNoticeFrom` | `cron` | Comportamento fixo: cron, o padrão: o aviso sai pela unidade do balcão do empréstimo (docs/specs/circulacao/avisos.md, item 6). |
| `OverduesBlockCirc` | `noblock` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `OverduesBlockRenewing` | `allow` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `PatronAutoComplete` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `PatronRestrictionTypes` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `PlaceHoldsOnOrdersFromSuggestions` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `PreventWithdrawingItemsStatus` | (vazio) | Comportamento fixo: Padrão do Koha (vazio): retirar é sempre permitido (lost-items.md, item 6). |
| `PrintNoticesMaxLines` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ProcessingFeeNote` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RandomizeHoldsQueueWeight` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RealTimeHoldsQueue` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RecallsMaxPickUpDelay` | `7` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RecordLocalUseOnReturn` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RecordStaffUserOnCheckout` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RefundLostOnReturnControl` | `CheckinLibrary` | Comportamento fixo: Padrão do Koha (CheckinLibrary): a regra vem da unidade que acha o exemplar (lost-items.md, item 10). |
| `RenewAccruingItemInOpac` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RenewAccruingItemWhenPaid` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RenewalSendNotice` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RentalFeesCheckoutConfirmation` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ReservesControlBranch` | `PatronLibrary` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ReservesMaxPickUpDelay` | `7` | Comportamento fixo: 7 dias quando holds_pickup_period não diz outra coisa (docs/specs/circulacao/reservas.md, item 15). |
| `ReservesNeedReturns` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `RestrictionBlockRenewing` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ReturnBeforeExpiry` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SCOAllowCheckin` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SCOBatchCheckoutsValidCategories` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SCOLoadCheckoutsByDefault` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SCOUserCSS` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SCOUserJS` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SIP2AddOpacMessagesToScreenMessage` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SIP2SortBinMapping` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SelfCheckAllowByIPRanges` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SelfCheckInModule` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SelfCheckInTimeout` | `120` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SelfCheckInUserCSS` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SelfCheckInUserJS` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SelfCheckReceiptPrompt` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SelfCheckTimeout` | `120` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SelfCheckoutByLogin` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ShowAllCheckins` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `ShowPatronImageInWebBasedSelfCheck` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SkipHoldTrapOnNotForLoanValue` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SpecifyDueDate` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SpecifyReturnDate` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `StaffSearchResultsDisplayBranch` | `holdingbranch` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `StaticHoldsQueueWeight` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `StockRotation` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `StoreLastBorrower` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SuspendHoldsIntranet` | `1` | Comportamento fixo: 1: a equipe suspende e retoma reservas na ficha do leitor (docs/specs/circulacao/reservas.md, item 19). |
| `SuspendHoldsOpac` | `1` | **`[circulation.holds] patrons_suspend`**. Ver docs/specs/opac/conta-do-leitor.md, item 25a. |
| `SuspensionsCalendar` | `noSuspensionsWhenClosed` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `SwitchOnSiteCheckouts` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `TitleHoldFeeStrategy` | `highest` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `TransferWhenCancelAllWaitingHolds` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `TransfersBlockCirc` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `TransfersMaxDaysWarning` | `3` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `TrapHoldsOnOrder` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UnseenRenewals` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UpdateItemLocationOnCheckin` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UpdateItemLocationOnCheckout` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UpdateItemLostStatusWhenPaid` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UpdateItemLostStatusWhenWriteoff` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UpdateItemWhenLostFromHoldList` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UpdateNotForLoanStatusOnCheckin` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UpdateNotForLoanStatusOnCheckout` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UpdateTotalIssuesOnCirc` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UseBranchTransferLimits` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UseCirculationDesks` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UseCourseReserves` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UseHoldsQueueFilterOptions` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UseLibraryFloatLimits` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UseLocationAsAQInSIP` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UseRecalls` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `UseTransportCostMatrix` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `WaitingNotifyAtCheckin` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `WaitingNotifyAtCheckout` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `WebBasedSelfCheck` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `WhenLostChargeReplacementFee` | `1` | Comportamento fixo: Padrão do Koha (ligado): marcar como perdido um exemplar emprestado cobra a reposição (lost-items.md, item 4). |
| `WhenLostForgiveFine` | `0` | Comportamento fixo: Padrão do Koha (desligado): a multa do exemplar perdido fica LOST, sem perdão (lost-items.md, item 4). |
| `canreservefromotherbranches` | `1` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `decreaseLoanHighHolds` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `decreaseLoanHighHoldsControl` | `static` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `decreaseLoanHighHoldsDuration` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `decreaseLoanHighHoldsIgnoreStatuses` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `decreaseLoanHighHoldsValue` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `emailLibrarianWhenHoldIsPlaced` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `finesCalendar` | `noFinesWhenClosed` | Regra de circulação: Junto com daysmode: decide se multas contam dias fechados. |
| `finesMode` | `off` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `itemBarcodeFallbackSearch` | `0` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `itemBarcodeInputFilter` | (vazio) | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `maxoutstanding` | `5` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `maxreserves` | `50` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `noissuescharge` | `5` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `numReturnedItemsToShow` | `20` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `previousIssuesDefaultSortOrder` | `asc` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `todaysIssuesDefaultSortOrder` | `desc` | A decidir: Paridade exigida: decidir uma a uma na spec de circulação (Fase 3). |
| `useDaysMode` | `Calendar` | Regra de circulação: Já existe como regra de circulação `daysmode` em $RULE_KINDS; a preferência vira o valor padrão. |
| `useDefaultReplacementCost` | `0` | Comportamento fixo: Padrão do Koha (desligado); fora desta versão (lost-items.md, §6). |

## E-resource management

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `ERMModule` | `0` | Fora da v1: ERM não entra na v1. |
| `ERMProviderEbscoApiKey` | (vazio) | Fora da v1: ERM não entra na v1. Senha ou chave: não é copiada. |
| `ERMProviderEbscoCustomerID` | (vazio) | Fora da v1: ERM não entra na v1. |
| `ERMProviders` | `local` | Fora da v1: ERM não entra na v1. |

## Enhanced content

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AllowMultipleCovers` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `AmazonAssocTag` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `AmazonCoverImages` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `AmazonLocale` | `US` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `Babeltheque` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `Babeltheque_url_js` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `Babeltheque_url_update` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `CoceHost` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `CoceProviders` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `CustomCoverImages` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `CustomCoverImagesURL` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `FRBRizeEditions` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `GoogleJackets` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `HTML5MediaEnabled` | `not` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `HTML5MediaExtensions` | `webm\|ogg\|ogv\|oga\|vtt` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `HTML5MediaYouTube` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `IntranetCoce` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `KohaManualBaseURL` | `https://koha-community.org/manual/` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `KohaManualLanguage` | `en` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `LibraryThingForLibrariesEnabled` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `LibraryThingForLibrariesID` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `LibraryThingForLibrariesTabbedView` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `LocalCoverImages` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `NovelistSelectEnabled` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `NovelistSelectPassword` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. Senha ou chave: não é copiada. |
| `NovelistSelectProfile` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `NovelistSelectStaffEnabled` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `NovelistSelectStaffProfile` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `NovelistSelectStaffView` | `tab` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `NovelistSelectView` | `tab` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OPACAmazonCoverImages` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OPACCustomCoverImages` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OPACFRBRizeEditions` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OPACLocalCoverImages` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OPACOverDrive` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OpacCoce` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OpenLibraryCovers` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OpenLibrarySearch` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OverDriveAuthName` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OverDriveCirculation` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OverDriveClientKey` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. Senha ou chave: não é copiada. |
| `OverDriveClientSecret` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. Senha ou chave: não é copiada. |
| `OverDriveLibraryID` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OverDrivePasswordRequired` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OverDriveUsername` | `cardnumber` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `OverDriveWebsiteID` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsAuthorNotes` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsAwards` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsClientCode` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsCoverImageSize` | `MC` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsCoverImages` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsEditions` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsEnabled` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsExcerpt` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsReviews` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsSeries` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsSummary` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `SyndeticsTOC` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `TagsEnabled` | `1` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `TagsExternalDictionary` | (vazio) | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `TagsInputOnDetail` | `1` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `TagsInputOnList` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `TagsModeration` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `TagsShowOnDetail` | `10` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `TagsShowOnList` | `6` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |
| `ThingISBN` | `0` | Fora da v1: Serviços externos de enriquecimento (capas, resenhas) ficam para depois da v1. |

## I18N/L10N

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AddressFormat` | `us` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface], com idiomas e formatos de data), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `CalendarFirstDayOfWeek` | `0` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface] primeiro_dia_semana), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `FacetSortingLocale` | `default` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface], com idiomas e formatos de data), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `OPACLanguages` | `en` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface] idiomas_opac), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `StaffInterfaceLanguages` | `en` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface], com idiomas e formatos de data), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `TimeFormat` | `24hr` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface] formato_hora), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `TranslateNotices` | `0` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface], com idiomas e formatos de data), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `alphabet` | `A B C D E F G H I J K L M N O P Q R S T U V W X Y Z` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface], com idiomas e formatos de data), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `dateformat` | `us` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface] formato_data), ainda sem campo: fora do padrão, vai para [koha_legacy]. |
| `opaclanguagesdisplay` | `0` | Campo do `elzevir.toml`: Prevista para o elzevir.toml ([interface], com idiomas e formatos de data), ainda sem campo: fora do padrão, vai para [koha_legacy]. |

## Interlibrary loans

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AutoILLBackendPriority` | (vazio) | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `CirculateILL` | `0` | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLCheckAvailability` | `0` | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLDefaultStaffEmail` | (vazio) | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLHiddenRequestStatuses` | (vazio) | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLHistoryCheck` | `0` | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLModule` | `0` | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLModuleDisclaimerByType` | (vazio) | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLModuleUnmediated` | `0` | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLOpacUnauthenticatedRequest` | `0` | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLOpacbackends` | (vazio) | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLPartnerCode` | `IL` | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLRequestsTabs` | (vazio) | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |
| `ILLSendStaffNotices` | (vazio) | Fora da v1: Empréstimo entre bibliotecas não entra na v1. |

## Logging

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AcquisitionLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `AdditionalContentLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `ApiKeyLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `AuthFailureLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `AuthSuccessLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `AuthoritiesLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `BorrowersLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `CardnumberLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `CataloguingLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `ClaimsLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `CronjobLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `DumpSearchQueryTemplate` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `DumpTemplateVarsIntranet` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `DumpTemplateVarsOpac` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `FinesLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `HoldsLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `IllLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `IssueLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `NoticesLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `RecallsLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `RenewalLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `ReportsLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `ReturnLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `SubscriptionLog` | `1` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `SuggestionsLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |
| `TransfersLog` | `0` | Comportamento fixo: O audit_log registra toda escrita; não há o que ligar ou desligar. |

## OPAC

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AllowPatronToSetCheckoutsVisibilityForGuarantor` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `AllowPatronToSetFinesVisibilityForGuarantor` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `AnonSuggestions` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `AnonymousPatron` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `AuthorityXSLTOpacDetailsDisplay` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `AuthorityXSLTOpacResultsDisplay` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `BiblioDefaultView` | `normal` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `BlockExpiredPatronOpacActions` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `COinSinOPACResults` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `DisplayOPACiconsXSLT` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `EmailAddressForPatronRegistrations` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `EmailPatronRegistrations` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `EnableOpacSearchHistory` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `EnableZotero` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `FineNoRenewalsIncludeCredits` | `1` | Comportamento fixo: Fixo ligado, como o padrão do Koha: o saldo desconta créditos não usados (docs/specs/circulacao/renovacao.md). |
| `LibraryName` | (vazio) | **`[oai] repository_name`**. O nome que o Identify do OAI-PMH informa; docs/specs/opac/oai-pmh.md, item 2. |
| `MaxOpenSuggestions` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `MaxTotalSuggestions` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `NotHighlightedWords` | `and\|or\|not` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `NumberOfSuggestionDays` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACAcquisitionDetails` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACAuthorIdentifiersAndInformation` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACBaseURL` | (vazio) | **`[oai] base_url`**. Com /oai no fim, o endereço que o Identify do OAI-PMH informa; docs/specs/opac/oai-pmh.md, item 2. |
| `OPACComments` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACDetailQRCode` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACDisableSendList` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACDisplay856uAsImage` | `OFF` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACFallback` | `prog` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACFinesTab` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACHoldRequests` | `1` | Comportamento fixo: Fixo ligado: o leitor reserva pela própria conta, com as regras do balcão (docs/specs/opac/conta-do-leitor.md). |
| `OPACHoldsHistory` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACISBD` | `#100\|\|{ 100a }{ 100b }{ 100c }{ 100d }{ 110a }{ 110b }{ 110c…` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACItemLocation` | `callnum` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACLoginLabelTextContent` | `cardnumber` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACMySummaryHTML` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACNoResultsFound` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACOpenURLItemTypes` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACPatronDetails` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACPlayMusicalInscripts` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACPopupAuthorsSearch` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACPrivacy` | `0` | Comportamento fixo: Fixo ligado: o leitor escolhe se o histórico de leitura é guardado (docs/specs/opac/conta-do-leitor.md). |
| `OPACReportProblem` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACResultsLibrary` | `homebranch` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACResultsMaxItems` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACResultsMaxItemsUnavailable` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACResultsUnavailableGroupingBy` | `branch` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACSearchForTitleIn` | `<a href="https://worldcat.org/search?q={TITLE}" target="_bla…` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACShelfBrowser` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACShibOnly` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACShowCheckoutName` | `0` | Comportamento fixo: Desligada: o OPAC nunca mostra quem tem o exemplar (docs/specs/opac/busca-e-registro.md, itens 7 e 13; LGPD). |
| `OPACShowHoldQueueDetails` | `none` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACShowLibraries` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACShowMusicalInscripts` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACShowOpenURL` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACShowSavings` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACShowUnusedAuthorities` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACSuggestionAutoFill` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACSuggestionMandatoryFields` | `title` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACSuggestionUnwantedFields` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACTableColExpandedByDefault` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACURLOpenInNewWindow` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACUserCSS` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACUserJS` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACUserSummary` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACViewOthersSuggestions` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACVirtualCard` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACVirtualCardBarcode` | `code39` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACXSLTDetailsDisplay` | `default` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACXSLTListsDisplay` | `default` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACXSLTResultsDisplay` | `default` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OPACpatronimages` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacAddMastheadLibraryPulldown` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacAdditionalStylesheet` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacAdvSearchMoreOptions` | `pubdate,itemtype,language,subtype,sorting,location` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacAdvSearchOptions` | `pubdate,itemtype,language,sorting,location` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacAdvancedSearchTypes` | `itemtypes` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacAllowPublicListCreation` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacAllowSharingPrivateLists` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacAuthorities` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacBrowseResults` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacBrowseSearch` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacBrowser` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacCatalogConcerns` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacCloud` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacExportOptions` | `bibtex,dc,marcxml,marc8,utf8,marcstd,mods,ris,isbd` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacFavicon` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacHiddenItems` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacHiddenItemsExceptions` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacHiddenItemsHidesRecord` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacHighlightedWords` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacHoldNotes` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacKohaUrl` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacLangSelectorMode` | `both` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacLocationBranchToDisplay` | `holding` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacLocationOnDetail` | `holding` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacMaintenance` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacMaxItemsToDisplay` | `50` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacMetaDescription` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacMetaRobots` | `noindex,nofollow` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacNewsLibrarySelect` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacNoItemTypeImages` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacPasswordChange` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacPublic` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacRenewalAllowed` | `1` | Comportamento fixo: Fixo ligado, como o padrão do Koha: o leitor renova pela própria conta (docs/specs/opac/conta-do-leitor.md). |
| `OpacRenewalBranch` | `checkoutbranch` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacResetPassword` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacSeparateHoldings` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacSeparateHoldingsBranch` | `homebranch` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacShowRecentComments` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacStarRatings` | `all` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacSuggestionManagedBy` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacTopissue` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpacTrustedCheckout` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpenURLImageLocation` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpenURLResolverURL` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `OpenURLText` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfModificationBorrowerUnwantedField` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfModificationMandatoryField` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistration` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationAgeRestriction` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationAlert` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationBorrowerMandatoryField` | `surname\|firstname` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationBorrowerUnwantedField` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationConfirmEmail` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationDefaultCategory` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationEmailMustBeUnique` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationExpireTemporaryAccountsDelay` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationLibraryList` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationPrefillForm` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PatronSelfRegistrationVerifyByEmail` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `PurgeListShareInvitesOlderThan` | `14` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `QuoteOfTheDay` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `Reference_NFL_Statuses` | `1\|2` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `RestrictedPageContent` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `RestrictedPageLocalIPs` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `RestrictedPageTitle` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `SearchMyLibraryFirst` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `ShelfBrowserUsesCcode` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `ShelfBrowserUsesHomeBranch` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `ShelfBrowserUsesLocation` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `ShowReviewer` | `full` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `ShowReviewerPhoto` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `SocialNetworks` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `TrackClicks` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `hidelostitems` | `0` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `numSearchRSSResults` | `50` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `opacbookbag` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `opaclayoutstylesheet` | `opac.css` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `opacreadinghistory` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `opacthemes` | `bootstrap` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `opacuserlogin` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `suggestion` | `1` | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |
| `suggestionPatronCategoryExceptions` | (vazio) | A decidir: Decidir na spec do OPAC: aparência vai para tokens de design, comportamento para elzevir.toml ou fixo. |

## Patrons

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AllowPatronToControlAutorenewal` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `AllowStaffToSetCheckoutsVisibilityForGuarantor` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `AllowStaffToSetFinesVisibilityForGuarantor` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `AnonymizeLastBorrower` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `AnonymizeLastBorrowerDays` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `AutoApprovePatronProfileSettings` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `AutoEmailNewUser` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `AutoRenewalNotices` | `preferences` | A decidir: Decidir na spec de leitores (Fase 2). |
| `AutomaticEmailReceipts` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `BorrowerMandatoryField` | `surname\|cardnumber` | A decidir: Decidir na spec de leitores (Fase 2). |
| `BorrowerRenewalPeriodBase` | `now` | A decidir: Decidir na spec de leitores (Fase 2). |
| `BorrowerUnwantedField` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `BorrowersTitles` | `Mr\|Mrs\|Miss\|Ms` | A decidir: Decidir na spec de leitores (Fase 2). |
| `CardnumberLength` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `CheckPrevCheckout` | `hardno` | A decidir: Decidir na spec de leitores (Fase 2). |
| `CheckPrevCheckoutDelay` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `CollapseFieldsPatronAddForm` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `CookieConsent` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `CookieConsentedJS` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `DefaultPatronSearchFields` | `firstname\|preferred_name\|middle_name\|surname\|othernames\|card…` | A decidir: Decidir na spec de leitores (Fase 2). |
| `DefaultPatronSearchMethod` | `starts_with` | A decidir: Decidir na spec de leitores (Fase 2). |
| `EmailFieldPrecedence` | `email\|emailpro\|B_email` | A decidir: Decidir na spec de leitores (Fase 2). |
| `EmailFieldPrimary` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `EmailFieldSelection` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `EmailSMSSendDriverFromAddress` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `EnableBorrowerFiles` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `EnableExpiredPasswordReset` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `EnhancedMessagingPreferences` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `EnhancedMessagingPreferencesOPAC` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `ExtendedPatronAttributes` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `FailedLoginAttempts` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `FallbackToSMSIfNoEmail` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `FeeOnChangePatronCategory` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `ForcePasswordResetWhenSetByStaff` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `IntranetReadingHistoryHolds` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `ListOwnerDesignated` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `ListOwnershipUponPatronDeletion` | `delete` | A decidir: Decidir na spec de leitores (Fase 2). |
| `LockExpiredDelay` | (vazio) | Comportamento fixo: Sem bloqueio de conta: o leitor vencido não entra no OPAC pela validade, e a anonimização segue [privacy] (docs/specs/circulacao/lgpd.md, item 14). |
| `MaxFine` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `MembershipExpiryDaysNotice` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `NotifyBorrowerDeparture` | `30` | A decidir: Decidir na spec de leitores (Fase 2). |
| `NotifyPasswordChange` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `PatronAgeRestriction` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `PatronAnonymizeDelay` | (vazio) | **`[privacy] anonymize_expired_days`**. Sem exigir conta bloqueada; docs/specs/circulacao/lgpd.md, item 14. |
| `PatronDuplicateMatchingAddFields` | `surname\|firstname\|dateofbirth` | A decidir: Decidir na spec de leitores (Fase 2). |
| `PatronQuickAddFields` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `PatronRemovalDelay` | (vazio) | Comportamento fixo: O leitor anonimizado nunca é apagado: fica para as estatísticas e para a conta (docs/specs/circulacao/lgpd.md, item 16). |
| `PatronsPerPage` | `20` | A decidir: Decidir na spec de leitores (Fase 2). |
| `PhoneNotification` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `PrefillGuaranteeField` | `phone,email,streetnumber,address,city,state,zipcode,country` | A decidir: Decidir na spec de leitores (Fase 2). |
| `PrivacyPolicyConsent` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `PrivacyPolicyURL` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `ProtectSuperlibrarianPrivileges` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `Pseudonymization` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `PseudonymizationPatronFields` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `PseudonymizationTransactionFields` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `RedirectGuaranteeEmail` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `RequireStrongPassword` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `RestrictPatronsWithFailedNotices` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `SMSSendAdditionalOptions` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `SMSSendDriver` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `SMSSendMaxChar` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `SMSSendPassword` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). Senha ou chave: não é copiada. |
| `SMSSendUsername` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `ShowPatronFirstnameIfDifferentThanPreferredname` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `StatisticsFields` | `location\|itype\|ccode` | A decidir: Decidir na spec de leitores (Fase 2). |
| `TalkingTechItivaPhoneNotification` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `TrackLastPatronActivityTriggers` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `UnsubscribeReflectionDelay` | (vazio) | A decidir: Decidir na spec de leitores (Fase 2). |
| `UpdateExpiryDateOnCategoryChange` | `softyes` | A decidir: Decidir na spec de leitores (Fase 2). |
| `UpdateMessagingPrefsOnCategoryChange` | `softyes` | A decidir: Decidir na spec de leitores (Fase 2). |
| `autoMemberNum` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `autoMemberNumValue` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `borrowerRelationship` | `father\|mother` | A decidir: Decidir na spec de leitores (Fase 2). |
| `intranetreadinghistory` | `1` | A decidir: Decidir na spec de leitores (Fase 2). |
| `minPasswordLength` | `8` | A decidir: Decidir na spec de leitores (Fase 2). |
| `patronimages` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `uppercasesurnames` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |
| `useDischarge` | `0` | A decidir: Decidir na spec de leitores (Fase 2). |

## Preservation

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `PreservationModule` | `0` | Fora da v1: Preservação não entra na v1. |
| `PreservationNotForLoanDefaultTrainIn` | (vazio) | Fora da v1: Preservação não entra na v1. |
| `PreservationNotForLoanWaitingListIn` | (vazio) | Fora da v1: Preservação não entra na v1. |

## Searching

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AdvancedSearchLanguages` | (vazio) | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `AdvancedSearchTypes` | `itemtypes` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `AuthorLinkSortBy` | `default` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `AuthorLinkSortOrder` | `asc` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `BiblioItemtypeInfo` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `BrowseResultSelection` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `DisplayLibraryFacets` | `holding` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `ElasticsearchBoostFieldMatch` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `ElasticsearchBoostFieldMatchAmount` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `ElasticsearchCrossFields` | `1` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `ElasticsearchEnableZebraQueue` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `ElasticsearchEscapeCharacters` | (vazio) | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `ElasticsearchPreventAutoTruncate` | `barcode\|control-number\|control-number-identifier\|date-of-acq…` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `EnableSearchHistory` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `FacetLabelTruncationLength` | `20` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `FacetMaxCount` | `20` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `FacetOrder` | `Alphabetical` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `IncludeSeeAlsoFromInSearches` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `IncludeSeeFromInSearches` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `IntranetNumbersPreferPhrase` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `LoadSearchHistoryToTheFirstLoggedUser` | `1` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `MaxSearchResultsItemsPerRecordStatusCheck` | `20` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `OPACNumbersPreferPhrase` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `OPACdefaultSortField` | `relevance` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `OPACdefaultSortOrder` | `dsc` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `OPACnumSearchResults` | `20` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `OPACnumSearchResultsDropdown` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `OpacElasticsearchEscapeCharacters` | (vazio) | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `PassItemMarcToXSLT` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `QueryAutoTruncate` | `1` | **`[search] complete_words`**. Ver docs/specs/catalogo/busca.md, item 5a. |
| `QueryFuzzy` | `1` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `QueryRegexEscapeOptions` | `escape` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `QueryStemming` | `1` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `QueryWeightFields` | `1` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `RedirectToSoleResult` | `1` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `RetainCatalogSearchTerms` | `1` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `RetainPatronsSearchTerms` | `1` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `SavedSearchFilters` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `SearchCancelledAndInvalidISBNandISSN` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `SearchLimitLibrary` | `homebranch` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `SearchWithISBNVariations` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `SearchWithISSNVariations` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `ShowHeadingUse` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `TraceCompleteSubfields` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `TraceSubjectSubdivisions` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `UNIMARCAuthorsFacetsSeparator` | `, ` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `UseICUStyleQuotes` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `defaultSortField` | `relevance` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `defaultSortOrder` | `dsc` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `displayFacetCount` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `expandedSearchOption` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `maxItemsInSearchResults` | `20` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `maxRecordsForFacets` | `20` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `numSearchResults` | `20` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `numSearchResultsDropdown` | `0` | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |
| `z3950Status` | (vazio) | A decidir: A busca do Elzevir é Tantivy; decidir na spec de busca (Fase 1) o que vira opção. |

## Serials

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `DisplayPublishedDate` | `1` | Fora da v1: Seriados não entram na v1. |
| `OPACSerialIssueDisplayCount` | `3` | Fora da v1: Seriados não entram na v1. |
| `PreserveSerialNotes` | `1` | Fora da v1: Seriados não entram na v1. |
| `RenewSerialAddsSuggestion` | `0` | Fora da v1: Seriados não entram na v1. |
| `RoutingListAddReserves` | `0` | Fora da v1: Seriados não entram na v1. |
| `RoutingSerials` | `1` | Fora da v1: Seriados não entram na v1. |
| `SerialsDefaultEmailAddress` | (vazio) | Fora da v1: Seriados não entram na v1. |
| `SerialsDefaultReplyTo` | (vazio) | Fora da v1: Seriados não entram na v1. |
| `SerialsSearchResultsLimit` | (vazio) | Fora da v1: Seriados não entram na v1. |
| `StaffSerialIssueDisplayCount` | `3` | Fora da v1: Seriados não entram na v1. |
| `SubscriptionDuplicateDroppedInput` | (vazio) | Fora da v1: Seriados não entram na v1. |
| `SubscriptionHistory` | `simplified` | Fora da v1: Seriados não entram na v1. |
| `makePreviousSerialAvailable` | `0` | Fora da v1: Seriados não entram na v1. |
| `opacSerialDefaultTab` | `subscriptions` | Fora da v1: Seriados não entram na v1. |

## Staff interface

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AudioAlerts` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `AuthorityXSLTDetailsDisplay` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `AuthorityXSLTResultsDisplay` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `Display856uAsImage` | `OFF` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `DisplayIconsXSLT` | `1` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `FilterSearchResultsByLoggedInBranch` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `HidePatronName` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `IntranetAddMastheadLibraryPulldown` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `IntranetCatalogSearchPulldown` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `IntranetCirculationHomeHTML` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `IntranetFavicon` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `IntranetNav` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `IntranetSlipPrinterJS` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `IntranetUserCSS` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `IntranetUserJS` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `ReportsExportFormatODS` | `1` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `ReportsExportLimit` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `StaffHighlightedWords` | `1` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `StaffLangSelectorMode` | `footer` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `TwoFactorAuthentication` | `disabled` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `UseWYSIWYGinSystemPreferences` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `XSLTDetailsDisplay` | `default` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `XSLTListsDisplay` | `default` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `XSLTResultsDisplay` | `default` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `intranet_includes` | `includes` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `intranetbookbag` | `1` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `intranetcolorstylesheet` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `intranetstylesheet` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `showLastPatron` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `showLastPatronCount` | `10` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `staffClientBaseURL` | (vazio) | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `staffShibOnly` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `template` | `prog` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `viewISBD` | `1` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `viewLabeledMARC` | `0` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |
| `viewMARC` | `1` | Interface: Interface da equipe redesenhada; preferências viram tokens de design ou somem. |

## Tools

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AdditionalContentsEditor` | `tinymce` | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `AllNoticeCSS` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `AllNoticeStylesheet` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `BarcodeSeparators` | `\s\r\n` | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `EmailNoticeCSS` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `EmailNoticeStylesheet` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `ImageLimit` | `5` | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `MaxItemsToDisplayForBatchDel` | `1000` | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `MaxItemsToDisplayForBatchMod` | `1000` | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `MaxItemsToProcessForBatchMod` | `1000` | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `NewsAuthorDisplay` | `none` | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `PrintNoticeCSS` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `PrintNoticeStylesheet` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `PrintSlipCSS` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `PrintSlipStylesheet` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |
| `UploadPurgeTemporaryFilesDays` | (vazio) | A decidir: Decidir por ferramenta, quando ela for especificada. |

## Web services

| Preferência | Padrão do Koha | No Elzevir |
|---|---|---|
| `AutoShareWithMana` | `subscription` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `ILS-DI` | `0` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `ILS-DI:AuthorizedIPs` | (vazio) | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `IdRef` | `0` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `Mana` | `2` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `ManaToken` | (vazio) | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). Senha ou chave: não é copiada. |
| `OAI-PMH` | `0` | **`[oai] enabled`**. Ver docs/specs/opac/oai-pmh.md, item 2. |
| `RESTAPIRenewalBranch` | `apiuserbranch` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `RESTBasicAuth` | `0` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `RESTOAuth2ClientCredentials` | `0` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `RESTPublicAPI` | `1` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `RESTPublicAnonymousRequests` | `1` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
| `SvcMaxReportRows` | `10` | A decidir: Decidir na spec de cada serviço (OAI-PMH, SRU, REST). |
