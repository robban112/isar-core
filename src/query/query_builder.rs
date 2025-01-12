use super::index_where_clause::IndexWhereClause;
use crate::collection::IsarCollection;
use crate::error::{illegal_arg, Result};
use crate::index::index_key::IndexKey;
use crate::object::isar_object::Property;
use crate::query::filter::Filter;
use crate::query::id_where_clause::IdWhereClause;
use crate::query::link_where_clause::LinkWhereClause;
use crate::query::where_clause::WhereClause;
use crate::query::{Query, Sort};

pub struct QueryBuilder<'a> {
    collection: &'a IsarCollection,
    where_clauses: Option<Vec<WhereClause>>,
    filter: Option<Filter>,
    sort: Vec<(Property, Sort)>,
    distinct: Vec<(Property, bool)>,
    offset: usize,
    limit: usize,
}

impl<'a> QueryBuilder<'a> {
    pub(crate) fn new(collection: &'a IsarCollection) -> QueryBuilder {
        QueryBuilder {
            collection,
            where_clauses: None,
            filter: None,
            sort: vec![],
            distinct: vec![],
            offset: 0,
            limit: usize::MAX,
        }
    }

    fn init_where_clauses(&mut self) {
        if self.where_clauses.is_none() {
            self.where_clauses = Some(vec![]);
        }
    }

    pub fn add_id_where_clause(&mut self, start: i64, end: i64) -> Result<()> {
        self.init_where_clauses();
        let (lower, upper, sort) = if start > end {
            (end, start, Sort::Descending)
        } else {
            (start, end, Sort::Ascending)
        };
        let wc = IdWhereClause::new(self.collection.db, lower, upper, sort);
        if !wc.is_empty() {
            self.where_clauses
                .as_mut()
                .unwrap()
                .push(WhereClause::Id(wc))
        }
        Ok(())
    }

    pub fn add_index_where_clause(
        &mut self,
        index_index: usize,
        start: IndexKey,
        include_start: bool,
        end: IndexKey,
        include_end: bool,
        skip_duplicates: bool,
    ) -> Result<()> {
        let index = self.collection.get_index_by_index(index_index)?;
        let (mut lower, include_lower, mut upper, include_upper, sort) = if start > end {
            (end, include_end, start, include_start, Sort::Descending)
        } else {
            (start, include_start, end, include_end, Sort::Ascending)
        };

        self.init_where_clauses();

        if (!include_lower && !lower.increase()) || (!include_upper && !upper.decrease()) {
            return Ok(());
        }
        let wc = IndexWhereClause::new(
            self.collection.db,
            index.clone(),
            lower,
            upper,
            skip_duplicates,
            sort,
        )?;
        self.where_clauses
            .as_mut()
            .unwrap()
            .push(WhereClause::Index(wc));

        Ok(())
    }

    pub fn add_link_where_clause(&mut self, link_index: usize, id: i64) -> Result<()> {
        self.add_link_where_clause_internal(self.collection, link_index, id)
    }

    pub fn add_backlink_where_clause(
        &mut self,
        col: &IsarCollection,
        link_index: usize,
        id: i64,
    ) -> Result<()> {
        self.add_link_where_clause_internal(col, link_index, id)
    }

    fn add_link_where_clause_internal(
        &mut self,
        col: &IsarCollection,
        link_index: usize,
        id: i64,
    ) -> Result<()> {
        let link = col.get_link(link_index)?;
        self.init_where_clauses();
        let wc = LinkWhereClause::new(link, id)?;
        self.where_clauses
            .as_mut()
            .unwrap()
            .push(WhereClause::Link(wc));
        Ok(())
    }

    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = Some(filter);
    }

    pub fn add_sort(&mut self, property: Property, sort: Sort) -> Result<()> {
        if property.data_type.is_scalar() {
            self.sort.push((property, sort));
            Ok(())
        } else {
            illegal_arg("Only scalar types may be used for sorting.")
        }
    }

    pub fn add_distinct(&mut self, property: Property, case_sensitive: bool) {
        self.distinct.push((property, case_sensitive));
    }

    pub fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
    }

    pub fn build(mut self) -> Query {
        if self.where_clauses.is_none() {
            self.add_id_where_clause(i64::MIN, i64::MAX).unwrap();
        }
        Query::new(
            self.collection.instance_id,
            self.where_clauses.unwrap(),
            self.filter,
            self.sort,
            self.distinct,
            self.offset,
            self.limit,
        )
    }
}
