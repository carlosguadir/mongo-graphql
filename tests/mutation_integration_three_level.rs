#[cfg(feature = "integration")]
mod common;

#[cfg(feature = "integration")]
mod tests {
    use fake::faker::lorem::en::Word;
    use fake::faker::name::en::Name;
    use fake::Fake;
    use mongodb::bson::oid::ObjectId;

    use mongo_graphql::executor;

    use crate::common::{get_db, get_schema};

    fn unique_alias(prefix: &str, label: &str) -> String {
        let word: String = Word().fake();
        let suffix = &ObjectId::new().to_hex()[..6];
        format!("{}_{}_{}_{}", prefix, label, word, suffix)
    }

    fn unique_name(prefix: &str, label: &str) -> String {
        let name: String = Name().fake();
        let suffix = &ObjectId::new().to_hex()[..6];
        format!("{}_{}_{}_{}", prefix, label, name, suffix)
    }

    #[tokio::test]
    async fn three_level_nested_mutations() {
        let db = get_db().await;
        let schema = get_schema().await;

        let prefix = format!("3L_{}", ObjectId::new().to_hex());

        // ── Notes on 3-level chain constraints ──
        //
        // A 3-level chain requires the Level 2 CreateWithout input to expose
        // nested fields. These come from two sources:
        //
        // 1. Forward relations NOT pointing to source (kept as nested inputs
        //    since build_create_without_fields only strips source-pointing relations).
        // 2. Reverse OneToMany/OneToOne fields added by add_reverse_fields_to_without
        //    from OTHER collections, excluding those originating from source.
        //
        // Chains using reverse fields (members/missions on Team) are blocked when
        // the reverse originates from the same collection as level 1 — they'd form
        // a circular type reference. All forward-relation-only chains are valid.

        // ═══════════════════════════════════════════════════════════════
        // Chain: Power → heroes.create → archenemy.create
        // L1: Power, L2: Hero (ManyToMany via hero_power), L3: Villain (FK archenemy_id)
        // HeroCreateWithoutPowerInput keeps archenemy as nested type (Villain ≠ Power).
        // ═══════════════════════════════════════════════════════════════
        {
            let power_name = unique_name(&prefix, "P_H_V_create");
            let l2_alias = unique_alias(&prefix, "P_H_V_Hero");
            let l2_identity: String = Name().fake();
            let villain_alias = unique_alias(&prefix, "P_H_V_Villain");
            let villain_identity: String = Name().fake();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createPower(input: {{
                            name: "{}",
                            type: "elemental",
                            level: 1,
                            created_at: "2024-01-01T00:00:00Z",
                            heroes: {{ create: [{{
                                alias: "{}",
                                secret_identity: "{}",
                                power_level: 700,
                                active: true,
                                joined_at: "2024-01-01T00:00:00Z",
                                created_at: "2024-01-01T00:00:00Z",
                                archenemy: {{ create: {{
                                    alias: "{}",
                                    secret_identity: "{}",
                                    threat_level: 90,
                                    active: true,
                                    created_at: "2024-01-01T00:00:00Z"
                                }} }}
                            }}] }}
                        }}) {{ id }}
                    }}"#,
                    power_name, l2_alias, l2_identity, villain_alias, villain_identity
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!(
                    "Power → heroes.create → archenemy.create failed: {:?}",
                    errors
                );
            }

            let power_oid =
                ObjectId::parse_str(result["data"]["createPower"]["id"].as_str().unwrap())
                    .unwrap();

            let junction_coll = db.collection::<mongodb::bson::Document>("hero_power");
            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let l2_doc = hero_coll
                .find_one(mongodb::bson::doc! { "alias": &l2_alias })
                .await
                .unwrap()
                .expect("L2 hero should exist");
            let junction = junction_coll
                .find_one(mongodb::bson::doc! { "hero_id": l2_doc.get_object_id("_id").unwrap() })
                .await
                .unwrap()
                .expect("hero_power junction should exist");
            assert_eq!(
                junction.get_object_id("power_id").unwrap(),
                power_oid,
                "junction.power_id should match L1 power"
            );

            let villain_coll = db.collection::<mongodb::bson::Document>("villain");
            let l3_doc = villain_coll
                .find_one(mongodb::bson::doc! { "alias": &villain_alias })
                .await
                .unwrap()
                .expect("L3 villain should exist");
            assert_eq!(
                l2_doc.get_object_id("archenemy_id").unwrap(),
                l3_doc.get_object_id("_id").unwrap(),
                "hero.archenemy_id should match L3 villain"
            );
        }

        // ═══════════════════════════════════════════════════════════════
        // Chain: Power → heroes.create → archenemy.connect
        // L1: Power, L2: Hero (ManyToMany via hero_power), L3: connect existing Villain
        // ═══════════════════════════════════════════════════════════════
        {
            let villain_oid = ObjectId::new();
            let villain_alias = unique_alias(&prefix, "P_H_V_connect");
            db.collection::<mongodb::bson::Document>("villain")
                .insert_one(mongodb::bson::doc! {
                    "_id": villain_oid,
                    "id": villain_oid,
                    "alias": &villain_alias,
                    "secret_identity": "Pre-existing Nemesis",
                    "threat_level": 95,
                    "active": true,
                    "created_at": mongodb::bson::DateTime::now(),
                })
                .await
                .unwrap();

            let power_name = unique_name(&prefix, "P_H_V_connect");
            let l2_alias = unique_alias(&prefix, "P_H_V_HeroC");
            let l2_identity: String = Name().fake();

            let result = executor::execute(
                schema,
                &format!(
                    r#"mutation {{
                        createPower(input: {{
                            name: "{}",
                            type: "elemental",
                            level: 1,
                            created_at: "2024-01-01T00:00:00Z",
                            heroes: {{ create: [{{
                                alias: "{}",
                                secret_identity: "{}",
                                power_level: 700,
                                active: true,
                                joined_at: "2024-01-01T00:00:00Z",
                                created_at: "2024-01-01T00:00:00Z",
                                archenemy: {{ connect: {{ id: "{}" }} }}
                            }}] }}
                        }}) {{ id }}
                    }}"#,
                    power_name, l2_alias, l2_identity, villain_oid.to_hex()
                ),
                async_graphql::Variables::default(),
                None,
                None,
            )
            .await
            .unwrap();

            if let Some(errors) = result.get("errors") {
                panic!(
                    "Power → heroes.create → archenemy.connect failed: {:?}",
                    errors
                );
            }

            let hero_coll = db.collection::<mongodb::bson::Document>("hero");
            let l2_doc = hero_coll
                .find_one(mongodb::bson::doc! { "alias": &l2_alias })
                .await
                .unwrap()
                .expect("L2 hero should exist");
            assert_eq!(
                l2_doc.get_object_id("archenemy_id").unwrap(),
                villain_oid,
                "hero.archenemy_id should match connected villain"
            );
        }
    }
}
