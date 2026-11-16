# Navegação entre catálogo e equipe em endereços separados

## Comportamento

A política de acesso pode receber os endereços efetivos dos dois listeners
por `with_listener_addresses`. Só habilita navegação quando ambas as portas
são diferentes de zero. Sem essa opção, mantém as recusas descritas em
[Isolamento HTTP](isolamento-http.md).

- GET `/equipe` no catálogo redireciona para `/equipe` da equipe com HTTP 307.
- GET de rota pública no endereço da equipe redireciona para o catálogo.
- Caminho e query são preservados; o destino usa a porta efetivamente vinculada.
- Em modo hostname, usa os nomes configurados. Em listeners separados, usa
  o IP específico do destino; para bind curinga, usa o Host validado da
  requisição. IPv6 mantém os colchetes. Não usa cabeçalhos Forwarded.
- O destino relativo ao protocolo preserva HTTP/HTTPS usado pelo navegador.
- POST e subrotas da equipe no catálogo continuam recusados. Nenhum handler
  da equipe é executado no catálogo. Autenticação, ACL do listener da equipe
  e cabeçalhos de segurança continuam aplicáveis.
- Host desconhecido em modo hostname mantém 404; Host inválido ou duplicado
  usado para formar destino mantém 400. Assets continuam compartilhados.

## Origem e escopo

Melhoria operacional, sem paridade com Koha. Complementa as políticas já
existentes para permitir os links relativos da interface em instalações
separadas. Esta entrega disponibiliza a opção na biblioteca web; configuração
e criação dos listeners pelo executável ficam para a integração posterior.

## Verificação

Testes de middleware conferem Location, query, IPv4/IPv6, portas reais,
portas zero, métodos recusados, Host inválido e manutenção do isolamento.
