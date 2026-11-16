# Políticas de acesso HTTP — base da P4

## Comportamento

A camada web oferece AccessPolicy validada e restrict_access para envolver um
router: endereço compartilhado, nomes de host separados, somente catálogo ou
somente equipe com lista opcional de até 32 IPs/redes CIDR IPv4/IPv6.

No modo por nome, nomes DNS ASCII são normalizados e precisam ser distintos.
Host inválido/repetido ou divergente da URI absoluta é recusado. Rotas da equipe
(/equipe e subrotas, incluindo POST) não atendem no host do catálogo. Assets
estáticos atendem nos dois nomes; hosts desconhecidos não alcançam handlers.

No modo por listener, catálogo recusa equipe e equipe recusa catálogo. A lista
de IPs usa exclusivamente ConnectInfo do peer TCP; ausência de peer com lista
ativa recusa acesso. IPv4 mapeado em IPv6 respeita as redes IPv4. Cabeçalhos
Forwarded e X-Forwarded-* não mudam a política. Respostas mantêm headers de segurança.

## Origem e paridade

Não porta comportamento do Koha. Concretiza as fronteiras do doc 04 §5. Este
recorte fornece a biblioteca; ativação por TOML, listeners, navegação entre
endereços e painel são integrações posteriores. O router padrão continua
compartilhado. A política não substitui autenticação, CSRF ou permissões.

## Verificação

Cobre Host desconhecido/repetido, portas inválidas, URI divergente, caminhos
codificados, GET/POST, assets, redes inválidas, limites de lista, IPv4 mapeado,
IPv6 e tentativa de contornar a restrição com cabeçalhos de proxy.
